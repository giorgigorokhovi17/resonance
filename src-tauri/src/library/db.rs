//! SQLite storage for the library and playlists.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

use super::media::{Backdrop, MediaKind};
use super::model::Track;

const SCHEMA_VERSION: i64 = 1;

pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA foreign_keys=ON;
         PRAGMA temp_store=MEMORY;
         PRAGMA cache_size=-16000;",
    )?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS folders(
            id INTEGER PRIMARY KEY,
            path TEXT NOT NULL UNIQUE,
            added_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS tracks(
            id INTEGER PRIMARY KEY,
            folder_id INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
            path TEXT NOT NULL UNIQUE,
            dir TEXT NOT NULL,
            title TEXT NOT NULL,
            artist TEXT NOT NULL,
            album TEXT NOT NULL,
            album_artist TEXT NOT NULL,
            genre TEXT NOT NULL,
            year INTEGER,
            track_no INTEGER,
            disc_no INTEGER,
            duration REAL NOT NULL,
            sample_rate INTEGER,
            bit_depth INTEGER,
            channels INTEGER,
            bitrate INTEGER,
            codec TEXT NOT NULL,
            file_size INTEGER NOT NULL,
            mtime INTEGER NOT NULL,
            cover_id TEXT,
            rg_track_gain REAL,
            rg_track_peak REAL,
            rg_album_gain REAL,
            rg_album_peak REAL,
            added_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_tracks_folder ON tracks(folder_id);
         CREATE INDEX IF NOT EXISTS idx_tracks_dir ON tracks(dir);
         CREATE TABLE IF NOT EXISTS playlists(
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS playlist_items(
            playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
            position INTEGER NOT NULL,
            path TEXT NOT NULL,
            PRIMARY KEY(playlist_id, position)
         );",
    )?;
    conn.execute(
        "INSERT INTO meta(key, value) VALUES('schema_version', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![SCHEMA_VERSION.to_string()],
    )?;
    // v2: user-hidden artwork, playlist description + custom cover.
    add_column(conn, "tracks", "cover_hidden", "INTEGER NOT NULL DEFAULT 0")?;
    add_column(conn, "playlists", "description", "TEXT NOT NULL DEFAULT ''")?;
    add_column(conn, "playlists", "cover_id", "TEXT")?;
    // v3: per-song artwork choices keyed by path, so rescans never overwrite them and a restored
    // backup applies as soon as the files are scanned. Moves the old tracks.cover_hidden flags here.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS track_art(
            path TEXT PRIMARY KEY,
            cover_id TEXT,
            hidden INTEGER NOT NULL DEFAULT 0
         );
         INSERT OR IGNORE INTO track_art(path, hidden) SELECT path, 1 FROM tracks WHERE cover_hidden = 1;
         UPDATE tracks SET cover_hidden = 0 WHERE cover_hidden = 1;",
    )?;
    // v4: where an imported playlist came from, so re-importing it updates it instead of adding a
    // duplicate. NULL for playlists made in the app.
    add_column(conn, "playlists", "source", "TEXT")?;
    add_column(conn, "playlists", "source_id", "TEXT")?;
    conn.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_playlists_source ON playlists(source, source_id)
         WHERE source IS NOT NULL AND source_id IS NOT NULL;",
    )?;
    // v5: ISRC and MusicBrainz recording id from the tags, for playlist matching and CSV export.
    // Tracks scanned before this version have neither: invalidate their mtime once so the next
    // (incremental) scan re-reads their tags. Artwork is reused, so this is cheap.
    let added = add_column(conn, "tracks", "isrc", "TEXT")?;
    add_column(conn, "tracks", "mbid", "TEXT")?;
    if added {
        conn.execute("UPDATE tracks SET mtime = -1", [])?;
    }
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_tracks_isrc ON tracks(isrc) WHERE isrc IS NOT NULL;
         CREATE INDEX IF NOT EXISTS idx_tracks_mbid ON tracks(mbid) WHERE mbid IS NOT NULL;",
    )?;
    // v6: playlist matcher.
    // - track_match_fts: normalised title/artist words per track (rowid = track id), to narrow
    //   candidates; track_match_state records which version of each track (mtime) and of the
    //   normalisation rules it was indexed with, so it's kept in sync incrementally.
    // - match_refs: the external reference behind an imported playlist item, by ref key.
    // - match_decisions: what the user decided for a reference, so it is never asked twice.
    // - playlist_items.ref_key: which reference an item came from (NULL for items added in the app).
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS track_match_fts USING fts5(title, artist, tokenize = 'unicode61');
         CREATE TABLE IF NOT EXISTS track_match_state(
            track_id INTEGER PRIMARY KEY,
            mtime INTEGER NOT NULL,
            norm INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS match_refs(
            ref_key TEXT PRIMARY KEY,
            ref_json TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS match_decisions(
            ref_key TEXT PRIMARY KEY,
            decision TEXT NOT NULL CHECK (decision IN ('accept', 'missing', 'skip')),
            path TEXT,
            decided_at INTEGER NOT NULL
         );",
    )?;
    add_column(conn, "playlist_items", "ref_key", "TEXT")?;
    // The matcher's parsed readings of each track (JSON), so opening it doesn't re-parse 50k titles.
    add_column(conn, "track_match_state", "readings", "TEXT")?;
    // v7: custom photos, GIFs and videos (library/media.rs). `media` describes each stored file;
    // animated covers keep their still poster in `cover_id` and the moving file in `motion_id`.
    // Playlists can also have a background with display options (dim NULL = the mode's default).
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS media(
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            mime TEXT NOT NULL,
            width INTEGER NOT NULL,
            height INTEGER NOT NULL,
            duration_ms INTEGER,
            bytes INTEGER NOT NULL,
            poster_id TEXT,
            added_at INTEGER NOT NULL
         );",
    )?;
    add_column(conn, "track_art", "motion_id", "TEXT")?;
    add_column(conn, "playlists", "motion_id", "TEXT")?;
    add_column(conn, "playlists", "background_id", "TEXT")?;
    add_column(conn, "playlists", "background_fit", "TEXT NOT NULL DEFAULT 'cover'")?;
    add_column(conn, "playlists", "background_dim", "REAL")?;
    add_column(conn, "playlists", "background_blur", "REAL NOT NULL DEFAULT 0")?;
    // v8: listening history for the stats page (library/stats.rs), and lyrics found online. Plays
    // keep the song's names as they were, so stats survive rescans and removed files. Music files
    // are never written to, so found lyrics are stored here, by path.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS plays(
            id INTEGER PRIMARY KEY,
            path TEXT NOT NULL,
            title TEXT NOT NULL,
            artist TEXT NOT NULL,
            album TEXT NOT NULL,
            album_artist TEXT NOT NULL,
            played_at INTEGER NOT NULL,
            listened REAL NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_plays_at ON plays(played_at);
         CREATE TABLE IF NOT EXISTS saved_lyrics(
            path TEXT PRIMARY KEY,
            lrc TEXT NOT NULL,
            source TEXT NOT NULL,
            saved_at INTEGER NOT NULL
         );",
    )?;
    // v9: songs the user deleted from disk (they went to the Recycle Bin), so they can be put back.
    // Names are kept as they were; playlists, artwork choices and lyrics stay keyed by path and
    // reconnect when the file is restored.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS deleted_tracks(
            id INTEGER PRIMARY KEY,
            path TEXT NOT NULL,
            title TEXT NOT NULL,
            artist TEXT NOT NULL,
            album TEXT NOT NULL,
            duration REAL NOT NULL,
            cover_id TEXT,
            was_readonly INTEGER NOT NULL DEFAULT 0,
            deleted_at INTEGER NOT NULL
         );",
    )?;
    Ok(())
}

/// Add a column if it's missing. Returns whether it was added.
fn add_column(conn: &Connection, table: &str, column: &str, decl: &str) -> Result<bool> {
    let exists = conn
        .prepare(&format!("PRAGMA table_info({table})"))?
        .query_map([], |r| r.get::<_, String>(1))?
        .filter_map(|c| c.ok())
        .any(|c| c == column);
    if !exists {
        conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"), [])?;
    }
    Ok(!exists)
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: i64,
    pub path: String,
    pub track_count: i64,
}

pub fn folders(conn: &Connection) -> Result<Vec<Folder>> {
    let mut stmt = conn.prepare(
        "SELECT f.id, f.path, (SELECT COUNT(*) FROM tracks t WHERE t.folder_id = f.id)
         FROM folders f ORDER BY f.path COLLATE NOCASE",
    )?;
    let rows = stmt
        .query_map([], |r| Ok(Folder { id: r.get(0)?, path: r.get(1)?, track_count: r.get(2)? }))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn add_folder(conn: &Connection, path: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO folders(path, added_at) VALUES(?1, ?2) ON CONFLICT(path) DO NOTHING",
        params![path, now()],
    )?;
    Ok(conn.query_row("SELECT id FROM folders WHERE path = ?1", params![path], |r| r.get(0))?)
}

pub fn remove_folder(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM folders WHERE id = ?1", params![id])?;
    Ok(())
}

const TRACK_COLS: &str = "t.id, t.path, t.title, t.artist, t.album, t.album_artist, t.genre, t.year, t.track_no, \
    t.disc_no, t.duration, t.sample_rate, t.bit_depth, t.channels, t.bitrate, t.codec, t.file_size, t.mtime, \
    CASE WHEN a.hidden = 1 THEN NULL ELSE COALESCE(a.cover_id, t.cover_id) END, \
    t.rg_track_gain, t.rg_track_peak, t.rg_album_gain, t.rg_album_peak, t.added_at, COALESCE(a.hidden, 0), \
    a.cover_id IS NOT NULL, t.isrc, t.mbid, CASE WHEN a.hidden = 1 THEN NULL ELSE a.motion_id END";

/// Tracks joined with the user's artwork choices.
const TRACK_FROM: &str = "tracks t LEFT JOIN track_art a ON a.path = t.path";

fn row_to_track(r: &Row) -> rusqlite::Result<Track> {
    Ok(Track {
        id: r.get(0)?,
        path: r.get(1)?,
        title: r.get(2)?,
        artist: r.get(3)?,
        album: r.get(4)?,
        album_artist: r.get(5)?,
        genre: r.get(6)?,
        year: r.get(7)?,
        track_no: r.get(8)?,
        disc_no: r.get(9)?,
        duration: r.get(10)?,
        sample_rate: r.get(11)?,
        bit_depth: r.get(12)?,
        channels: r.get(13)?,
        bitrate: r.get(14)?,
        codec: r.get(15)?,
        file_size: r.get::<_, i64>(16)? as u64,
        mtime: r.get(17)?,
        cover_id: r.get(18)?,
        rg_track_gain: r.get(19)?,
        rg_track_peak: r.get(20)?,
        rg_album_gain: r.get(21)?,
        rg_album_peak: r.get(22)?,
        added_at: r.get(23)?,
        cover_hidden: r.get::<_, i64>(24)? != 0,
        custom_cover: r.get(25)?,
        isrc: r.get(26)?,
        mbid: r.get(27)?,
        motion_id: r.get(28)?,
    })
}

/// Hide (or restore) artwork for tracks. Only affects the app; files are not modified.
pub fn set_cover_hidden(conn: &mut Connection, ids: &[i64], hidden: bool) -> Result<()> {
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO track_art(path, hidden) SELECT path, ?2 FROM tracks WHERE id = ?1
             ON CONFLICT(path) DO UPDATE SET hidden = excluded.hidden",
        )?;
        for id in ids {
            stmt.execute(params![id, hidden as i64])?;
        }
        tx.execute("DELETE FROM track_art WHERE cover_id IS NULL AND hidden = 0", [])?;
    }
    tx.commit()?;
    Ok(())
}

/// Use `cover_id` as the artwork for these tracks (or with `None`, go back to their own artwork).
/// `motion_id` is a GIF or video to show instead where artwork animates; `cover_id` is then its
/// still poster. Setting artwork also un-hides it.
pub fn set_custom_cover(conn: &mut Connection, ids: &[i64], cover_id: Option<&str>, motion_id: Option<&str>) -> Result<()> {
    let motion_id = cover_id.and(motion_id);
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO track_art(path, cover_id, motion_id, hidden) SELECT path, ?2, ?3, 0 FROM tracks WHERE id = ?1
             ON CONFLICT(path) DO UPDATE SET cover_id = excluded.cover_id, motion_id = excluded.motion_id,
                hidden = CASE WHEN excluded.cover_id IS NULL THEN track_art.hidden ELSE 0 END",
        )?;
        for id in ids {
            stmt.execute(params![id, cover_id, motion_id])?;
        }
        tx.execute("DELETE FROM track_art WHERE cover_id IS NULL AND hidden = 0", [])?;
    }
    tx.commit()?;
    Ok(())
}

/// A per-song artwork choice, by file path.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TrackArt {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_id: Option<String>,
    /// GIF or video shown where artwork animates (`cover_id` is its poster).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion_id: Option<String>,
    #[serde(default)]
    pub hidden: bool,
}

pub fn track_art(conn: &Connection) -> Result<Vec<TrackArt>> {
    let mut stmt = conn.prepare("SELECT path, cover_id, motion_id, hidden FROM track_art")?;
    let rows = stmt
        .query_map([], |r| {
            Ok(TrackArt { path: r.get(0)?, cover_id: r.get(1)?, motion_id: r.get(2)?, hidden: r.get::<_, i64>(3)? != 0 })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn put_track_art(conn: &mut Connection, art: &[TrackArt]) -> Result<()> {
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO track_art(path, cover_id, motion_id, hidden) VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET cover_id = excluded.cover_id, motion_id = excluded.motion_id,
                hidden = excluded.hidden",
        )?;
        for a in art {
            stmt.execute(params![a.path, a.cover_id, a.cover_id.as_ref().and(a.motion_id.as_ref()), a.hidden as i64])?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn all_tracks(conn: &Connection) -> Result<Vec<Track>> {
    let mut stmt = conn.prepare(&format!("SELECT {TRACK_COLS} FROM {TRACK_FROM}"))?;
    let rows = stmt.query_map([], row_to_track)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Tracks by id, returned in the order of `ids` (unknown ids are skipped).
pub fn tracks_by_ids(conn: &Connection, ids: &[i64]) -> Result<Vec<Track>> {
    let mut stmt = conn.prepare_cached(&format!("SELECT {TRACK_COLS} FROM {TRACK_FROM} WHERE t.id = ?1"))?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(t) = stmt.query_row(params![id], row_to_track).optional()? {
            out.push(t);
        }
    }
    Ok(out)
}

pub fn track_by_path(conn: &Connection, path: &str) -> Result<Option<Track>> {
    let mut stmt = conn.prepare_cached(&format!("SELECT {TRACK_COLS} FROM {TRACK_FROM} WHERE t.path = ?1"))?;
    Ok(stmt.query_row(params![path], row_to_track).optional()?)
}

pub struct Existing {
    pub id: i64,
    pub size: u64,
    pub mtime: i64,
    pub dir: String,
    pub album: String,
    pub cover_id: Option<String>,
    pub duration: f64,
}

pub fn existing_for_folder(conn: &Connection, folder_id: i64) -> Result<HashMap<String, Existing>> {
    let mut stmt =
        conn.prepare("SELECT path, id, file_size, mtime, dir, album, cover_id, duration FROM tracks WHERE folder_id = ?1")?;
    let mut map = HashMap::new();
    let rows = stmt.query_map(params![folder_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            Existing {
                id: r.get(1)?,
                size: r.get::<_, i64>(2)? as u64,
                mtime: r.get(3)?,
                dir: r.get(4)?,
                album: r.get(5)?,
                cover_id: r.get(6)?,
                duration: r.get(7)?,
            },
        ))
    })?;
    for row in rows {
        let (p, e) = row?;
        map.insert(p, e);
    }
    Ok(map)
}

pub fn upsert_tracks(conn: &mut Connection, folder_id: i64, tracks: &[(Track, String)]) -> Result<()> {
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO tracks(folder_id, path, dir, title, artist, album, album_artist, genre, year, track_no,
                disc_no, duration, sample_rate, bit_depth, channels, bitrate, codec, file_size, mtime, cover_id,
                rg_track_gain, rg_track_peak, rg_album_gain, rg_album_peak, added_at, isrc, mbid)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20,
                ?21, ?22, ?23, ?24, ?25, ?26, ?27)
             ON CONFLICT(path) DO UPDATE SET
                folder_id = excluded.folder_id, dir = excluded.dir, title = excluded.title,
                artist = excluded.artist, album = excluded.album, album_artist = excluded.album_artist,
                genre = excluded.genre, year = excluded.year, track_no = excluded.track_no,
                disc_no = excluded.disc_no, duration = excluded.duration, sample_rate = excluded.sample_rate,
                bit_depth = excluded.bit_depth, channels = excluded.channels, bitrate = excluded.bitrate,
                codec = excluded.codec, file_size = excluded.file_size, mtime = excluded.mtime,
                cover_id = excluded.cover_id, rg_track_gain = excluded.rg_track_gain,
                rg_track_peak = excluded.rg_track_peak, rg_album_gain = excluded.rg_album_gain,
                rg_album_peak = excluded.rg_album_peak, isrc = excluded.isrc, mbid = excluded.mbid",
        )?;
        let added = now();
        for (t, dir) in tracks {
            stmt.execute(params![
                folder_id,
                t.path,
                dir,
                t.title,
                t.artist,
                t.album,
                t.album_artist,
                t.genre,
                t.year,
                t.track_no,
                t.disc_no,
                t.duration,
                t.sample_rate,
                t.bit_depth,
                t.channels,
                t.bitrate,
                t.codec,
                t.file_size as i64,
                t.mtime,
                t.cover_id,
                t.rg_track_gain,
                t.rg_track_peak,
                t.rg_album_gain,
                t.rg_album_peak,
                added,
                t.isrc,
                t.mbid,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn delete_tracks(conn: &mut Connection, ids: &[i64]) -> Result<()> {
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare_cached("DELETE FROM tracks WHERE id = ?1")?;
        for id in ids {
            stmt.execute(params![id])?;
        }
    }
    tx.commit()?;
    Ok(())
}

// ---- recently deleted -------------------------------------------------------------------------

/// A song deleted from disk by the app (it went to the Recycle Bin).
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeletedTrack {
    pub id: i64,
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
    pub cover_id: Option<String>,
    pub was_readonly: bool,
    pub deleted_at: i64,
    /// Whether it can still be restored (filled in from the Recycle Bin, not stored).
    pub in_bin: bool,
}

pub fn add_deleted(conn: &Connection, t: &Track, was_readonly: bool, deleted_at: i64) -> Result<i64> {
    conn.execute(
        "INSERT INTO deleted_tracks(path, title, artist, album, duration, cover_id, was_readonly, deleted_at)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![t.path, t.title, t.artist, t.album, t.duration, t.cover_id, was_readonly as i64, deleted_at],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Most recently deleted first.
pub fn deleted_tracks(conn: &Connection) -> Result<Vec<DeletedTrack>> {
    let mut stmt = conn.prepare(
        "SELECT id, path, title, artist, album, duration, cover_id, was_readonly, deleted_at
         FROM deleted_tracks ORDER BY deleted_at DESC, id DESC",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(DeletedTrack {
                id: r.get(0)?,
                path: r.get(1)?,
                title: r.get(2)?,
                artist: r.get(3)?,
                album: r.get(4)?,
                duration: r.get(5)?,
                cover_id: r.get(6)?,
                was_readonly: r.get::<_, i64>(7)? != 0,
                deleted_at: r.get(8)?,
                in_bin: true,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn remove_deleted(conn: &Connection, ids: &[i64]) -> Result<()> {
    let mut stmt = conn.prepare_cached("DELETE FROM deleted_tracks WHERE id = ?1")?;
    for id in ids {
        stmt.execute(params![id])?;
    }
    Ok(())
}

// ---- playlists --------------------------------------------------------------------------------

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub track_count: i64,
    pub description: String,
    /// User-chosen playlist photo (the still poster when `motion_id` is set).
    pub cover_id: Option<String>,
    /// GIF or video used as the playlist's cover where artwork animates.
    pub motion_id: Option<String>,
    /// Photo, GIF or video behind the playlist's page.
    pub background: Option<Backdrop>,
    /// Artwork of the first few tracks, for a collage when there is no custom photo.
    pub preview_covers: Vec<String>,
    /// Import source ("m3u", "xspf", …) and its id there; `None` for playlists made in the app.
    pub source: Option<String>,
    pub source_id: Option<String>,
}

pub fn playlists(conn: &Connection) -> Result<Vec<Playlist>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.name, p.created_at, p.updated_at,
                (SELECT COUNT(*) FROM playlist_items i WHERE i.playlist_id = p.id),
                p.description, p.cover_id, p.source, p.source_id, p.motion_id,
                p.background_id, p.background_fit, p.background_dim, p.background_blur
         FROM playlists p ORDER BY p.name COLLATE NOCASE",
    )?;
    let mut rows = stmt
        .query_map([], |r| {
            let background = r.get::<_, Option<String>>(10)?.map(|media_id| -> rusqlite::Result<Backdrop> {
                Ok(Backdrop { media_id, fit: r.get(11)?, dim: r.get(12)?, blur: r.get(13)? })
            });
            Ok(Playlist {
                id: r.get(0)?,
                name: r.get(1)?,
                created_at: r.get(2)?,
                updated_at: r.get(3)?,
                track_count: r.get(4)?,
                description: r.get(5)?,
                cover_id: r.get(6)?,
                motion_id: r.get(9)?,
                background: background.transpose()?,
                preview_covers: Vec::new(),
                source: r.get(7)?,
                source_id: r.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut covers = conn.prepare_cached(
        "SELECT COALESCE(a.cover_id, t.cover_id) FROM playlist_items i JOIN tracks t ON t.path = i.path
         LEFT JOIN track_art a ON a.path = t.path
         WHERE i.playlist_id = ?1 AND COALESCE(a.cover_id, t.cover_id) IS NOT NULL AND COALESCE(a.hidden, 0) = 0
         ORDER BY i.position LIMIT 60",
    )?;
    for p in &mut rows {
        let ids = covers.query_map(params![p.id], |r| r.get::<_, String>(0))?;
        for id in ids.flatten() {
            if !p.preview_covers.contains(&id) {
                p.preview_covers.push(id);
                if p.preview_covers.len() == 4 {
                    break;
                }
            }
        }
    }
    Ok(rows)
}

pub fn update_playlist(conn: &Connection, id: i64, name: &str, description: &str) -> Result<()> {
    conn.execute(
        "UPDATE playlists SET name = ?2, description = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, name, description, now()],
    )?;
    Ok(())
}

/// Set (or with `None`, remove) a playlist's photo. `motion_id` is a GIF or video shown instead
/// where artwork animates; `cover_id` is then its still poster.
pub fn set_playlist_cover(conn: &Connection, id: i64, cover_id: Option<&str>, motion_id: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE playlists SET cover_id = ?2, motion_id = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, cover_id, cover_id.and(motion_id), now()],
    )?;
    Ok(())
}

/// Set (or with `None`, remove) the photo, GIF or video behind a playlist's page.
pub fn set_playlist_background(conn: &Connection, id: i64, background: Option<&Backdrop>) -> Result<()> {
    let (media, fit, dim, blur) = match background {
        Some(b) => (Some(b.media_id.as_str()), b.fit.as_str(), b.dim, b.blur),
        None => (None, "cover", None, 0.0),
    };
    conn.execute(
        "UPDATE playlists SET background_id = ?2, background_fit = ?3, background_dim = ?4, background_blur = ?5,
            updated_at = ?6 WHERE id = ?1",
        params![id, media, fit, dim, blur, now()],
    )?;
    Ok(())
}

pub fn create_playlist(conn: &Connection, name: &str) -> Result<i64> {
    let t = now();
    conn.execute("INSERT INTO playlists(name, created_at, updated_at) VALUES(?1, ?2, ?2)", params![name, t])?;
    Ok(conn.last_insert_rowid())
}

/// The playlist imported from `source`/`source_id`, if there is one: (id, name).
pub fn playlist_by_source(conn: &Connection, source: &str, source_id: &str) -> Result<Option<(i64, String)>> {
    Ok(conn
        .query_row(
            "SELECT id, name FROM playlists WHERE source = ?1 AND source_id = ?2",
            params![source, source_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

/// Record where a playlist was imported from. Another playlist holding the same key (possible
/// after restoring a backup) gives it up, so the key stays unique.
pub fn set_playlist_source(conn: &Connection, id: i64, source: &str, source_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE playlists SET source = NULL, source_id = NULL WHERE source = ?1 AND source_id = ?2 AND id != ?3",
        params![source, source_id, id],
    )?;
    conn.execute("UPDATE playlists SET source = ?2, source_id = ?3 WHERE id = ?1", params![id, source, source_id])?;
    Ok(())
}

pub fn rename_playlist(conn: &Connection, id: i64, name: &str) -> Result<()> {
    conn.execute("UPDATE playlists SET name = ?2, updated_at = ?3 WHERE id = ?1", params![id, name, now()])?;
    Ok(())
}

pub fn delete_playlist(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM playlists WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn playlist_paths(conn: &Connection, id: i64) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT path FROM playlist_items WHERE playlist_id = ?1 ORDER BY position")?;
    let rows = stmt.query_map(params![id], |r| r.get(0))?.collect::<Result<Vec<String>, _>>()?;
    Ok(rows)
}

/// Replace a playlist's contents with `paths` (in order). Used for edits in the app (reorder,
/// remove, add): items keep the reference they were imported from, matched up by path.
pub fn set_playlist_paths(conn: &mut Connection, id: i64, paths: &[String]) -> Result<()> {
    let mut old: HashMap<String, std::collections::VecDeque<String>> = HashMap::new();
    for (path, key) in playlist_items(conn, id)? {
        if let Some(k) = key {
            old.entry(path).or_default().push_back(k);
        }
    }
    let items: Vec<(String, Option<String>)> =
        paths.iter().map(|p| (p.clone(), old.get_mut(p).and_then(|q| q.pop_front()))).collect();
    set_playlist_items(conn, id, &items)
}

/// A playlist's items in order: (path, reference key).
pub fn playlist_items(conn: &Connection, id: i64) -> Result<Vec<(String, Option<String>)>> {
    let mut stmt = conn.prepare("SELECT path, ref_key FROM playlist_items WHERE playlist_id = ?1 ORDER BY position")?;
    let rows = stmt.query_map(params![id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Replace a playlist's items with `items` (path, reference key), in order.
pub fn set_playlist_items(conn: &mut Connection, id: i64, items: &[(String, Option<String>)]) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM playlist_items WHERE playlist_id = ?1", params![id])?;
    {
        let mut stmt =
            tx.prepare_cached("INSERT INTO playlist_items(playlist_id, position, path, ref_key) VALUES(?1, ?2, ?3, ?4)")?;
        for (i, (p, k)) in items.iter().enumerate() {
            stmt.execute(params![id, i as i64, p, k])?;
        }
    }
    tx.execute("UPDATE playlists SET updated_at = ?2 WHERE id = ?1", params![id, now()])?;
    tx.commit()?;
    Ok(())
}

// ---- custom media -----------------------------------------------------------------------------

/// A stored photo, GIF or video (see `media.rs`).
#[derive(Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Media {
    pub id: String,
    pub kind: MediaKind,
    pub mime: String,
    pub width: u32,
    pub height: u32,
    pub duration_ms: Option<u64>,
    pub bytes: u64,
    /// Still image in the cover cache, shown wherever the file isn't played.
    pub poster_id: Option<String>,
}

const MEDIA_COLS: &str = "id, kind, mime, width, height, duration_ms, bytes, poster_id";

/// `None` for a kind this version doesn't know.
fn row_to_media(r: &Row) -> rusqlite::Result<Option<Media>> {
    let Some(kind) = MediaKind::parse(&r.get::<_, String>(1)?) else { return Ok(None) };
    Ok(Some(Media {
        id: r.get(0)?,
        kind,
        mime: r.get(2)?,
        width: r.get(3)?,
        height: r.get(4)?,
        duration_ms: r.get::<_, Option<i64>>(5)?.map(|d| d.max(0) as u64),
        bytes: r.get::<_, i64>(6)?.max(0) as u64,
        poster_id: r.get(7)?,
    }))
}

pub fn put_media(conn: &Connection, m: &Media) -> Result<()> {
    conn.execute(
        "INSERT INTO media(id, kind, mime, width, height, duration_ms, bytes, poster_id, added_at)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(id) DO UPDATE SET kind = excluded.kind, mime = excluded.mime, width = excluded.width,
            height = excluded.height, duration_ms = excluded.duration_ms, bytes = excluded.bytes,
            poster_id = excluded.poster_id",
        params![m.id, m.kind.as_str(), m.mime, m.width, m.height, m.duration_ms.map(|d| d as i64), m.bytes as i64, m.poster_id, now()],
    )?;
    Ok(())
}

pub fn media(conn: &Connection) -> Result<Vec<Media>> {
    let mut stmt = conn.prepare(&format!("SELECT {MEDIA_COLS} FROM media ORDER BY added_at, id"))?;
    let rows = stmt.query_map([], row_to_media)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows.into_iter().flatten().collect())
}

pub fn media_by_id(conn: &Connection, id: &str) -> Result<Option<Media>> {
    let mut stmt = conn.prepare_cached(&format!("SELECT {MEDIA_COLS} FROM media WHERE id = ?1"))?;
    Ok(stmt.query_row(params![id], row_to_media).optional()?.flatten())
}

pub fn delete_media(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM media WHERE id = ?1", params![id])?;
    Ok(())
}

/// Media ids that songs and playlists use.
pub fn media_in_use(conn: &Connection) -> Result<HashSet<String>> {
    let mut out = HashSet::new();
    for sql in [
        "SELECT motion_id FROM track_art WHERE motion_id IS NOT NULL",
        "SELECT motion_id FROM playlists WHERE motion_id IS NOT NULL",
        "SELECT background_id FROM playlists WHERE background_id IS NOT NULL",
    ] {
        let mut stmt = conn.prepare(sql)?;
        for id in stmt.query_map([], |r| r.get::<_, String>(0))? {
            out.insert(id?);
        }
    }
    Ok(out)
}

/// Deletes media rows that nothing uses. `also_keep` holds ids used outside the database (the
/// app background in the settings). Returns the ids that remain.
pub fn prune_media(conn: &Connection, also_keep: &HashSet<String>) -> Result<HashSet<String>> {
    let mut keep = media_in_use(conn)?;
    keep.extend(also_keep.iter().cloned());
    let all: Vec<String> =
        conn.prepare("SELECT id FROM media")?.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<_, _>>()?;
    let mut remaining = HashSet::new();
    for id in all {
        if keep.contains(&id) {
            remaining.insert(id);
        } else {
            delete_media(conn, &id)?;
        }
    }
    Ok(remaining)
}

// ---- matcher storage --------------------------------------------------------------------------

/// Store references by key (JSON of `interchange::TrackRef`).
/// Lyrics found online for a song, with where they came from.
pub fn saved_lyrics(conn: &Connection, path: &str) -> Result<Option<(String, String)>> {
    Ok(conn
        .query_row("SELECT lrc, source FROM saved_lyrics WHERE path = ?1", params![path], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?)
}

pub fn save_lyrics(conn: &Connection, path: &str, lrc: &str, source: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO saved_lyrics(path, lrc, source, saved_at) VALUES(?1, ?2, ?3, ?4)
         ON CONFLICT(path) DO UPDATE SET lrc = excluded.lrc, source = excluded.source, saved_at = excluded.saved_at",
        params![path, lrc, source, now()],
    )?;
    Ok(())
}

pub fn delete_saved_lyrics(conn: &Connection, path: &str) -> Result<()> {
    conn.execute("DELETE FROM saved_lyrics WHERE path = ?1", params![path])?;
    Ok(())
}

pub fn put_match_refs(conn: &mut Connection, refs: &[(String, String)]) -> Result<()> {
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO match_refs(ref_key, ref_json) VALUES(?1, ?2) ON CONFLICT(ref_key) DO UPDATE SET ref_json = excluded.ref_json",
        )?;
        for (k, j) in refs {
            stmt.execute(params![k, j])?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn match_ref_json(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn.query_row("SELECT ref_json FROM match_refs WHERE ref_key = ?1", params![key], |r| r.get(0)).optional()?)
}

/// A stored decision: "accept" (with the chosen path), "missing" or "skip".
#[derive(Clone, Debug, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchDecision {
    pub decision: String,
    pub path: Option<String>,
    pub decided_at: i64,
}

pub fn match_decisions(conn: &Connection) -> Result<HashMap<String, MatchDecision>> {
    let mut stmt = conn.prepare("SELECT ref_key, decision, path, decided_at FROM match_decisions")?;
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, MatchDecision { decision: r.get(1)?, path: r.get(2)?, decided_at: r.get(3)? }))
    })?;
    Ok(rows.collect::<Result<HashMap<_, _>, _>>()?)
}

pub fn clear_match_decision(conn: &Connection, key: &str) -> Result<()> {
    conn.execute("DELETE FROM match_decisions WHERE ref_key = ?1", params![key])?;
    Ok(())
}

/// Point every playlist item that came from reference `key` at `path`. Returns the playlists
/// touched.
pub fn set_path_for_ref(conn: &Connection, key: &str, path: &str) -> Result<usize> {
    let ids: Vec<i64> = conn
        .prepare("SELECT DISTINCT playlist_id FROM playlist_items WHERE ref_key = ?1")?
        .query_map(params![key], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    conn.execute("UPDATE playlist_items SET path = ?2 WHERE ref_key = ?1", params![key, path])?;
    for id in &ids {
        conn.execute("UPDATE playlists SET updated_at = ?2 WHERE id = ?1", params![id, now()])?;
    }
    Ok(ids.len())
}

pub fn set_match_decision(conn: &Connection, key: &str, decision: &str, path: Option<&str>) -> Result<()> {
    conn.execute(
        "INSERT INTO match_decisions(ref_key, decision, path, decided_at) VALUES(?1, ?2, ?3, ?4)
         ON CONFLICT(ref_key) DO UPDATE SET decision = excluded.decision, path = excluded.path, decided_at = excluded.decided_at",
        params![key, decision, path, now()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_columns_migrate_and_force_one_reread() {
        let file = std::env::temp_dir().join(format!("db-migrate-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&file);
        {
            // A library from before v5: no isrc/mbid columns.
            let mut conn = open(&file).unwrap();
            let folder = add_folder(&conn, r"C:\Music").unwrap();
            let t = Track { path: r"C:\Music\a.flac".into(), title: "A".into(), mtime: 123, ..Default::default() };
            upsert_tracks(&mut conn, folder, &[(t, r"C:\Music".into())]).unwrap();
            conn.execute_batch(
                "DROP INDEX idx_tracks_isrc; DROP INDEX idx_tracks_mbid;
                 ALTER TABLE tracks DROP COLUMN isrc; ALTER TABLE tracks DROP COLUMN mbid;",
            )
            .unwrap();
        }
        let mut conn = open(&file).unwrap();
        assert_eq!(all_tracks(&conn).unwrap()[0].mtime, -1, "old rows are invalidated so the next scan re-reads their tags");

        // Values round-trip once the columns exist, and reopening doesn't invalidate again.
        let t = Track { path: r"C:\Music\a.flac".into(), title: "A".into(), mtime: 456, isrc: Some("USUM71601827".into()), mbid: Some("6c4e3b79-5345-4854-ac55-82ad1ec2d928".into()), ..Default::default() };
        upsert_tracks(&mut conn, 1, &[(t, r"C:\Music".into())]).unwrap();
        drop(conn);
        let conn = open(&file).unwrap();
        let back = all_tracks(&conn).unwrap().remove(0);
        assert_eq!((back.mtime, back.isrc.as_deref(), back.mbid.as_deref()), (456, Some("USUM71601827"), Some("6c4e3b79-5345-4854-ac55-82ad1ec2d928")));
        drop(conn);
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_file(file.with_extension("db-wal"));
        let _ = std::fs::remove_file(file.with_extension("db-shm"));
    }

    #[test]
    fn custom_artwork_and_hiding() {
        let mut conn = open(Path::new(":memory:")).unwrap();
        let folder = add_folder(&conn, r"C:\Music").unwrap();
        let t = Track { path: r"C:\Music\a.flac".into(), title: "A".into(), cover_id: Some("aa".into()), ..Default::default() };
        upsert_tracks(&mut conn, folder, &[(t.clone(), r"C:\Music".into())]).unwrap();
        let id = all_tracks(&conn).unwrap()[0].id;
        let get = |c: &Connection| all_tracks(c).unwrap().remove(0);

        set_custom_cover(&mut conn, &[id], Some("bb"), None).unwrap();
        let x = get(&conn);
        assert_eq!((x.cover_id.as_deref(), x.custom_cover, x.cover_hidden), (Some("bb"), true, false));

        // A rescan rewrites the file's own artwork but keeps the user's choice.
        upsert_tracks(&mut conn, folder, &[(Track { cover_id: Some("cc".into()), ..t.clone() }, r"C:\Music".into())]).unwrap();
        assert_eq!(get(&conn).cover_id.as_deref(), Some("bb"));

        set_cover_hidden(&mut conn, &[id], true).unwrap();
        let x = get(&conn);
        assert_eq!((x.cover_id.as_deref(), x.cover_hidden), (None, true));
        // Setting new artwork un-hides it.
        set_custom_cover(&mut conn, &[id], Some("dd"), None).unwrap();
        assert_eq!(get(&conn).cover_id.as_deref(), Some("dd"));

        set_custom_cover(&mut conn, &[id], None, None).unwrap();
        let x = get(&conn);
        assert_eq!((x.cover_id.as_deref(), x.custom_cover), (Some("cc"), false));
        assert!(track_art(&conn).unwrap().is_empty(), "no leftover rows once nothing is customized");

        // Playlist previews use the custom artwork too.
        set_custom_cover(&mut conn, &[id], Some("ee"), None).unwrap();
        let p = create_playlist(&conn, "P").unwrap();
        set_playlist_paths(&mut conn, p, &[t.path.clone()]).unwrap();
        assert_eq!(playlists(&conn).unwrap()[0].preview_covers, vec!["ee".to_string()]);
    }

    #[test]
    fn plays_and_saved_lyrics_tables_migrate() {
        let (conn, file) = reopen_after("v8", "DROP TABLE plays; DROP TABLE saved_lyrics;");
        assert_eq!(playlists(&conn).unwrap()[0].name, "Old", "older data is kept");
        save_lyrics(&conn, r"C:\Music\a.flac", "[00:01.00]Hi", "lrclib").unwrap();
        save_lyrics(&conn, r"C:\Music\a.flac", "[00:02.00]Hello", "lrclib").unwrap();
        assert_eq!(saved_lyrics(&conn, r"C:\Music\a.flac").unwrap(), Some(("[00:02.00]Hello".into(), "lrclib".into())));
        delete_saved_lyrics(&conn, r"C:\Music\a.flac").unwrap();
        assert_eq!(saved_lyrics(&conn, r"C:\Music\a.flac").unwrap(), None);
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM plays", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
        drop(conn);
        remove_db(file);
    }

    /// Opens a database file, lets `downgrade` turn it back into an older schema, and reopens it.
    fn reopen_after(name: &str, downgrade: &str) -> (Connection, std::path::PathBuf) {
        let file = std::env::temp_dir().join(format!("db-{name}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&file);
        {
            let conn = open(&file).unwrap();
            add_folder(&conn, r"C:\Music").unwrap();
            create_playlist(&conn, "Old").unwrap();
            conn.execute_batch(downgrade).unwrap();
        }
        (open(&file).unwrap(), file)
    }

    fn remove_db(file: std::path::PathBuf) {
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_file(file.with_extension("db-wal"));
        let _ = std::fs::remove_file(file.with_extension("db-shm"));
    }

    fn columns(conn: &Connection, table: &str) -> Vec<String> {
        conn.prepare(&format!("PRAGMA table_info({table})"))
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .map(|c| c.unwrap())
            .collect()
    }

    fn video(id: &str) -> Media {
        Media {
            id: id.into(),
            kind: MediaKind::Video,
            mime: "video/mp4".into(),
            width: 1280,
            height: 720,
            duration_ms: Some(8000),
            bytes: 1234,
            poster_id: Some("p1".into()),
        }
    }

    #[test]
    fn deleted_tracks_table_migrates() {
        let (conn, file) = reopen_after("deleted-tracks", "DROP TABLE deleted_tracks;");
        assert_eq!(playlists(&conn).unwrap()[0].name, "Old", "older data is kept");
        assert!(deleted_tracks(&conn).unwrap().is_empty());
        let t = Track { path: r"C:\Music.flac".into(), title: "A".into(), artist: "X".into(), duration: 61.5, cover_id: Some("c1".into()), ..Default::default() };
        let first = add_deleted(&conn, &t, true, 100).unwrap();
        let second = add_deleted(&conn, &Track { path: r"C:\Music\b.flac".into(), title: "B".into(), ..Default::default() }, false, 200).unwrap();
        let rows = deleted_tracks(&conn).unwrap();
        assert_eq!(rows.iter().map(|d| d.id).collect::<Vec<_>>(), vec![second, first], "newest first");
        let a = &rows[1];
        assert_eq!((a.path.as_str(), a.title.as_str(), a.artist.as_str(), a.duration), (r"C:\Music.flac", "A", "X", 61.5));
        assert_eq!((a.cover_id.as_deref(), a.was_readonly, a.deleted_at), (Some("c1"), true, 100));
        remove_deleted(&conn, &[first]).unwrap();
        assert_eq!(deleted_tracks(&conn).unwrap().len(), 1);
        drop(conn);
        // Reopening keeps the records.
        let conn = open(&file).unwrap();
        assert_eq!(deleted_tracks(&conn).unwrap()[0].id, second);
        drop(conn);
        remove_db(file);
    }

    #[test]
    fn media_table_migrates() {
        let (conn, file) = reopen_after("media-table", "DROP TABLE media;");
        assert!(media(&conn).unwrap().is_empty());
        let m = video("abc");
        put_media(&conn, &m).unwrap();
        assert_eq!(media_by_id(&conn, "abc").unwrap(), Some(m.clone()));
        // Storing the same file again updates it in place.
        put_media(&conn, &Media { poster_id: Some("p2".into()), ..m }).unwrap();
        assert_eq!(media(&conn).unwrap().len(), 1);
        assert_eq!(media_by_id(&conn, "abc").unwrap().unwrap().poster_id.as_deref(), Some("p2"));
        // A kind from a newer version is skipped, not an error.
        conn.execute("UPDATE media SET kind = 'hologram'", []).unwrap();
        assert!(media(&conn).unwrap().is_empty());
        drop(conn);
        remove_db(file);
    }

    #[test]
    fn track_motion_column_migrates() {
        let (mut conn, file) = reopen_after("track-motion", "ALTER TABLE track_art DROP COLUMN motion_id;");
        assert!(columns(&conn, "track_art").contains(&"motion_id".to_string()));
        let t = Track { path: r"C:\Music\a.flac".into(), title: "A".into(), ..Default::default() };
        upsert_tracks(&mut conn, 1, &[(t, r"C:\Music".into())]).unwrap();
        let id = all_tracks(&conn).unwrap()[0].id;
        set_custom_cover(&mut conn, &[id], Some("poster"), Some("loop")).unwrap();
        let x = all_tracks(&conn).unwrap().remove(0);
        assert_eq!((x.cover_id.as_deref(), x.motion_id.as_deref()), (Some("poster"), Some("loop")));
        assert_eq!(track_art(&conn).unwrap()[0].motion_id.as_deref(), Some("loop"));
        // Hidden artwork hides the motion too; a still cover or no cover clears it.
        set_cover_hidden(&mut conn, &[id], true).unwrap();
        assert_eq!(all_tracks(&conn).unwrap()[0].motion_id, None);
        set_custom_cover(&mut conn, &[id], Some("still"), None).unwrap();
        assert_eq!(all_tracks(&conn).unwrap()[0].motion_id, None);
        set_custom_cover(&mut conn, &[id], None, Some("loop")).unwrap();
        assert!(track_art(&conn).unwrap().is_empty(), "motion without a poster isn't kept");
        drop(conn);
        remove_db(file);
    }

    #[test]
    fn playlist_motion_column_migrates() {
        let (conn, file) = reopen_after("playlist-motion", "ALTER TABLE playlists DROP COLUMN motion_id;");
        let p = &playlists(&conn).unwrap()[0];
        assert_eq!((p.name.as_str(), p.motion_id.as_deref()), ("Old", None));
        set_playlist_cover(&conn, p.id, Some("poster"), Some("loop")).unwrap();
        let p = playlists(&conn).unwrap().remove(0);
        assert_eq!((p.cover_id.as_deref(), p.motion_id.as_deref()), (Some("poster"), Some("loop")));
        set_playlist_cover(&conn, p.id, None, Some("loop")).unwrap();
        assert_eq!(playlists(&conn).unwrap()[0].motion_id, None);
        drop(conn);
        remove_db(file);
    }

    #[test]
    fn playlist_background_columns_migrate() {
        let (conn, file) = reopen_after(
            "playlist-background",
            "ALTER TABLE playlists DROP COLUMN background_id; ALTER TABLE playlists DROP COLUMN background_fit;
             ALTER TABLE playlists DROP COLUMN background_dim; ALTER TABLE playlists DROP COLUMN background_blur;",
        );
        let p = playlists(&conn).unwrap().remove(0);
        assert_eq!(p.background, None, "existing playlists have no background");
        let bg = Backdrop { media_id: "vid".into(), fit: "contain".into(), dim: Some(0.4), blur: 12.0 };
        set_playlist_background(&conn, p.id, Some(&bg)).unwrap();
        assert_eq!(playlists(&conn).unwrap()[0].background, Some(bg));
        set_playlist_background(&conn, p.id, None).unwrap();
        assert_eq!(playlists(&conn).unwrap()[0].background, None);
        drop(conn);
        remove_db(file);
    }

    #[test]
    fn unused_media_is_pruned() {
        let mut conn = open(Path::new(":memory:")).unwrap();
        let folder = add_folder(&conn, r"C:\Music").unwrap();
        let t = Track { path: r"C:\Music\a.flac".into(), title: "A".into(), ..Default::default() };
        upsert_tracks(&mut conn, folder, &[(t, r"C:\Music".into())]).unwrap();
        for id in ["song", "cover", "bg", "skin", "orphan"] {
            put_media(&conn, &video(id)).unwrap();
        }
        let track = all_tracks(&conn).unwrap()[0].id;
        set_custom_cover(&mut conn, &[track], Some("p"), Some("song")).unwrap();
        let p = create_playlist(&conn, "P").unwrap();
        set_playlist_cover(&conn, p, Some("p"), Some("cover")).unwrap();
        set_playlist_background(&conn, p, Some(&Backdrop { media_id: "bg".into(), ..Default::default() })).unwrap();
        let skin: HashSet<String> = ["skin".to_string()].into_iter().collect();
        let mut left: Vec<_> = prune_media(&conn, &skin).unwrap().into_iter().collect();
        left.sort();
        assert_eq!(left, vec!["bg", "cover", "skin", "song"]);
        assert!(media_by_id(&conn, "orphan").unwrap().is_none());
        // Removing the playlist background frees it on the next pass.
        set_playlist_background(&conn, p, None).unwrap();
        assert!(!prune_media(&conn, &skin).unwrap().contains("bg"));
    }
}
