//! Backup / restore: settings, EQ presets, library folders, playlists and artwork choices in one
//! file, for moving to a new PC. Music files themselves are not included. Custom GIFs, videos and
//! background photos are, up to `MEDIA_ITEM_MAX` each and `MEDIA_TOTAL_MAX` in all; the restore
//! summary lists what was left out (its still poster is restored in its place).
//!
//! On restore, paths are remapped when the music lives somewhere else on the new PC (another drive
//! letter, user folder or library root): a missing folder is matched to an existing one under the
//! current library roots by its trailing path components ("…\Artist\Album").

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::commands::EqPreset;
use crate::library::db::{self, Media, TrackArt};
use crate::library::media::{self, Backdrop, MediaKind};
use crate::library::Library;
use crate::settings::Settings;

const FORMAT: &str = "resonance-backup";
const VERSION: u32 = 1;
/// Larger media files stay out of backups, which hold everything in one JSON file.
pub const MEDIA_ITEM_MAX: u64 = 25 * 1024 * 1024;
pub const MEDIA_TOTAL_MAX: u64 = 200 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    format: String,
    version: u32,
    created_at: i64,
    app_version: String,
    settings: Settings,
    #[serde(default)]
    eq_presets: Vec<EqPreset>,
    #[serde(default)]
    library_folders: Vec<String>,
    #[serde(default)]
    playlists: Vec<BackupPlaylist>,
    #[serde(default)]
    track_art: Vec<TrackArt>,
    /// Artwork images (full-size JPEG, base64) by cover id.
    #[serde(default)]
    artwork: BTreeMap<String, String>,
    /// Custom GIFs, videos and background photos by media id.
    #[serde(default)]
    media: BTreeMap<String, BackupMedia>,
    /// Media that was too large to include, for the restore summary.
    #[serde(default)]
    skipped_media: Vec<SkippedMedia>,
    /// UI preferences stored by the frontend (sort orders, filters, …), passed through as-is.
    #[serde(default)]
    ui: serde_json::Value,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupPlaylist {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    cover_id: Option<String>,
    paths: Vec<String>,
    /// Import source, so re-importing the file after a restore still updates this playlist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    motion_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    background: Option<Backdrop>,
    /// Left out of the sidebar.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    hidden: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupMedia {
    kind: MediaKind,
    mime: String,
    width: u32,
    height: u32,
    #[serde(default)]
    duration_ms: Option<u64>,
    #[serde(default)]
    poster_id: Option<String>,
    /// The file, base64.
    data: String,
}

/// A media file that was left out of a backup, and what used it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkippedMedia {
    /// "App background", "Background of “Chill”", "Artwork of “Song”".
    pub used_by: String,
    pub kind: MediaKind,
    pub bytes: u64,
    pub reason: String,
}

impl SkippedMedia {
    /// One line for the restore summary.
    fn describe(&self) -> String {
        let what = match self.kind {
            MediaKind::Image => "photo",
            MediaKind::Gif => "GIF",
            MediaKind::WebpAnimated => "animated WebP",
            MediaKind::Video => "video",
        };
        let mb = (self.bytes as f64 / (1024.0 * 1024.0)).max(0.1);
        format!("{} ({mb:.1} MB {what}): {}", self.used_by, self.reason)
    }
}

/// Which media files fit in a backup: each at most `per_item`, all together at most `total`.
/// Smaller files go first so as many as possible fit. Returns (included ids, skipped with reason).
pub fn plan_media(sizes: &[(String, u64)], per_item: u64, total: u64) -> (Vec<String>, Vec<(String, String)>) {
    let mut order: Vec<&(String, u64)> = sizes.iter().collect();
    order.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    let (mut included, mut skipped, mut used) = (Vec::new(), Vec::new(), 0u64);
    for (id, bytes) in order {
        if *bytes > per_item {
            skipped.push((id.clone(), format!("over {} MB, too large for a backup", per_item / (1024 * 1024))));
        } else if used + bytes > total {
            skipped.push((id.clone(), format!("the backup's {} MB limit for media was reached", total / (1024 * 1024))));
        } else {
            used += bytes;
            included.push(id.clone());
        }
    }
    (included, skipped)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSummary {
    pub playlists: usize,
    pub presets: usize,
    pub artwork: usize,
    /// Custom GIFs, videos and photos included, and left out for size.
    pub media: usize,
    pub media_skipped: usize,
    pub bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreSummary {
    pub playlists: usize,
    pub presets: usize,
    pub artwork: usize,
    pub media: usize,
    /// Custom media that wasn't in the backup (too large) or couldn't be restored.
    pub skipped_media: Vec<String>,
    pub folders_added: Vec<String>,
    /// Library folders from the backup that don't exist on this PC.
    pub folders_missing: Vec<String>,
    /// Playlist songs found at a different location than on the old PC.
    pub relocated: usize,
    /// Playlist songs that couldn't be found anywhere.
    pub unresolved: usize,
    pub ui: serde_json::Value,
    pub settings: Settings,
}

pub fn export(lib: &Library, settings: Settings, eq_presets: Vec<EqPreset>, ui: serde_json::Value, file: &Path) -> Result<ExportSummary, String> {
    let (library_folders, playlists, track_art) = {
        let conn = lib.db.lock();
        let folders = db::folders(&conn).map_err(err)?.into_iter().map(|f| f.path).collect::<Vec<_>>();
        let mut playlists = Vec::new();
        for p in db::playlists(&conn).map_err(err)? {
            let paths = db::playlist_paths(&conn, p.id).map_err(err)?;
            playlists.push(BackupPlaylist {
                name: p.name,
                description: p.description,
                cover_id: p.cover_id,
                paths,
                source: p.source,
                source_id: p.source_id,
                motion_id: p.motion_id,
                background: p.background,
                hidden: p.hidden,
            });
        }
        (folders, playlists, db::track_art(&conn).map_err(err)?)
    };

    // Custom media and what uses it, for the summary of anything left out.
    let mut uses: Vec<(String, String)> = Vec::new();
    {
        let conn = lib.db.lock();
        for a in &track_art {
            if let Some(m) = &a.motion_id {
                let title = db::track_by_path(&conn, &a.path).ok().flatten().map(|t| t.title).unwrap_or_else(|| {
                    Path::new(&a.path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
                });
                uses.push((m.clone(), format!("Artwork of “{title}”")));
            }
        }
    }
    for p in &playlists {
        if let Some(m) = &p.motion_id {
            uses.push((m.clone(), format!("Cover of “{}”", p.name)));
        }
        if let Some(b) = &p.background {
            uses.push((b.media_id.clone(), format!("Background of “{}”", p.name)));
        }
    }
    if let Some(b) = &settings.appearance.background {
        uses.push((b.media_id.clone(), "App background".into()));
    }
    let rows: HashMap<String, (Media, u64)> = {
        let conn = lib.db.lock();
        uses.iter()
            .filter_map(|(id, _)| {
                let m = db::media_by_id(&conn, id).ok().flatten()?;
                let (path, _) = lib.media.file(id)?;
                let bytes = std::fs::metadata(path).ok()?.len();
                Some((id.clone(), (m, bytes)))
            })
            .collect()
    };
    let sizes: Vec<(String, u64)> = rows.iter().map(|(id, (_, b))| (id.clone(), *b)).collect();
    let (included, skipped) = plan_media(&sizes, MEDIA_ITEM_MAX, MEDIA_TOTAL_MAX);
    let mut media_out = BTreeMap::new();
    for id in included {
        let (m, _) = &rows[&id];
        let Some(bytes) = lib.media.file(&id).and_then(|(p, _)| std::fs::read(p).ok()) else { continue };
        media_out.insert(
            id.clone(),
            BackupMedia {
                kind: m.kind,
                mime: m.mime.clone(),
                width: m.width,
                height: m.height,
                duration_ms: m.duration_ms,
                poster_id: m.poster_id.clone(),
                data: B64.encode(bytes),
            },
        );
    }
    let skipped_media: Vec<SkippedMedia> = skipped
        .iter()
        .flat_map(|(id, reason)| {
            let (m, bytes) = &rows[id];
            uses.iter().filter(move |(u, _)| u == id).map(move |(_, used_by)| SkippedMedia {
                used_by: used_by.clone(),
                kind: m.kind,
                bytes: *bytes,
                reason: reason.clone(),
            })
        })
        .collect();

    let mut artwork = BTreeMap::new();
    // Posters of included media too (a background's poster isn't referenced anywhere else).
    let ids = track_art
        .iter()
        .filter_map(|a| a.cover_id.as_ref())
        .chain(playlists.iter().filter_map(|p| p.cover_id.as_ref()))
        .chain(media_out.values().filter_map(|m| m.poster_id.as_ref()));
    for id in ids {
        if artwork.contains_key(id) {
            continue;
        }
        if let Some(bytes) = lib.covers.file(id, true).and_then(|f| std::fs::read(f).ok()) {
            artwork.insert(id.clone(), B64.encode(bytes));
        }
    }

    let mut settings = settings;
    settings.device_id = None; // device ids are specific to this PC
    let backup = Backup {
        format: FORMAT.into(),
        version: VERSION,
        created_at: db::now(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        settings,
        eq_presets,
        library_folders,
        playlists,
        track_art,
        artwork,
        media: media_out,
        skipped_media,
        ui,
    };
    let summary = (backup.playlists.len(), backup.eq_presets.len(), backup.artwork.len());
    let (media_count, media_skipped) = (backup.media.len(), backup.skipped_media.len());
    let bytes = serde_json::to_vec(&backup).map_err(err)?;
    let tmp = file.with_extension("tmp");
    std::fs::write(&tmp, &bytes).map_err(|e| format!("Cannot write {}: {e}", file.display()))?;
    std::fs::rename(&tmp, file).map_err(|e| format!("Cannot write {}: {e}", file.display()))?;
    Ok(ExportSummary {
        playlists: summary.0,
        presets: summary.1,
        artwork: summary.2,
        media: media_count,
        media_skipped,
        bytes: bytes.len() as u64,
    })
}

pub fn read(file: &Path) -> Result<Backup, String> {
    let bytes = std::fs::read(file).map_err(|e| format!("Cannot read {}: {e}", file.display()))?;
    let b: Backup = serde_json::from_slice(&bytes).map_err(|_| "That file isn't a Undertone backup.".to_string())?;
    if b.format != FORMAT {
        return Err("That file isn't a Undertone backup.".into());
    }
    if b.version > VERSION {
        return Err("This backup was made by a newer version of Undertone. Update the app first.".into());
    }
    Ok(b)
}

/// What `restore` changed; the caller applies settings/presets/folders to the live app.
pub struct Restored {
    pub summary: RestoreSummary,
    pub eq_presets: Vec<EqPreset>,
}

/// Restores library folders, artwork, artwork choices and playlists into the database.
/// `add_folder` adds a library folder (and starts scanning it).
pub fn restore(lib: &Library, b: Backup, mut add_folder: impl FnMut(&str) -> Result<(), String>) -> Result<Restored, String> {
    // 1. Library folders that exist here (and don't overlap a current one).
    let mut roots: Vec<String> = lib.folders().map_err(err)?.into_iter().map(|f| f.path).collect();
    let mut folders_added = Vec::new();
    let mut folders_missing = Vec::new();
    for f in &b.library_folders {
        let p = Path::new(f);
        if roots.iter().any(|r| p.starts_with(r) || Path::new(r).starts_with(p)) {
            continue;
        }
        if p.is_dir() {
            add_folder(f)?;
            roots.push(f.clone());
            folders_added.push(f.clone());
        } else {
            folders_missing.push(f.clone());
        }
    }

    // 2. Artwork images (re-stored, so ids are this PC's).
    let mut cover_map: HashMap<String, String> = HashMap::new();
    for (id, data) in &b.artwork {
        if let Some(new_id) = B64.decode(data).ok().and_then(|bytes| lib.covers.store(&bytes)) {
            cover_map.insert(id.clone(), new_id);
        }
    }

    // 2b. Custom media files, with their posters.
    let mut skipped_media: Vec<String> = b.skipped_media.iter().map(SkippedMedia::describe).collect();
    let mut media_map: HashMap<String, String> = HashMap::new();
    for (id, m) in &b.media {
        let restored = (|| {
            let bytes = B64.decode(&m.data).ok()?;
            let ext = media::ext_for_mime(&m.mime)?;
            let new_id = lib.media.put(&bytes, ext).ok()?;
            let row = Media {
                id: new_id.clone(),
                kind: m.kind,
                mime: m.mime.clone(),
                width: m.width,
                height: m.height,
                duration_ms: m.duration_ms,
                bytes: bytes.len() as u64,
                poster_id: m.poster_id.as_ref().and_then(|p| cover_map.get(p)).cloned(),
            };
            db::put_media(&lib.db.lock(), &row).ok()?;
            Some(new_id)
        })();
        match restored {
            Some(new_id) => {
                media_map.insert(id.clone(), new_id);
            }
            None => skipped_media.push(format!("A {} from the backup couldn't be restored", m.kind.as_str())),
        }
    }
    let map_backdrop = |bg: &Option<Backdrop>| -> Option<Backdrop> {
        let bg = bg.as_ref()?;
        let media_id = media_map.get(&bg.media_id)?.clone();
        Backdrop { media_id, ..bg.clone() }.normalized()
    };

    let mut mapper = PathMapper::new(&b.library_folders, &roots);

    // 3. Per-song artwork choices.
    let art: Vec<TrackArt> = b
        .track_art
        .iter()
        .filter_map(|a| {
            let cover_id = match &a.cover_id {
                Some(c) => Some(cover_map.get(c)?.clone()),
                None => None,
            };
            let path = mapper.map(&a.path).unwrap_or_else(|| a.path.clone());
            // Motion that wasn't in the backup falls back to its poster (the cover).
            let motion_id = a.motion_id.as_ref().and_then(|m| media_map.get(m)).cloned();
            Some(TrackArt { path, cover_id, motion_id, hidden: a.hidden })
        })
        .collect();
    db::put_track_art(&mut lib.db.lock(), &art).map_err(err)?;

    // 4. Playlists: replace ones with the same name, create the rest.
    let (mut relocated, mut unresolved) = (0, 0);
    {
        let mut conn = lib.db.lock();
        let existing = db::playlists(&conn).map_err(err)?;
        for p in &b.playlists {
            let paths: Vec<String> = p
                .paths
                .iter()
                .map(|old| match mapper.map(old) {
                    Some(new) => {
                        if &new != old {
                            relocated += 1;
                        }
                        new
                    }
                    None => {
                        unresolved += 1;
                        old.clone()
                    }
                })
                .collect();
            let id = match existing.iter().find(|e| e.name.eq_ignore_ascii_case(&p.name)) {
                Some(e) => e.id,
                None => db::create_playlist(&conn, &p.name).map_err(err)?,
            };
            db::set_playlist_paths(&mut conn, id, &paths).map_err(err)?;
            db::update_playlist(&conn, id, &p.name, &p.description).map_err(err)?;
            let cover = p.cover_id.as_ref().and_then(|c| cover_map.get(c));
            let motion = p.motion_id.as_ref().and_then(|m| media_map.get(m));
            db::set_playlist_cover(&conn, id, cover.map(|s| s.as_str()), motion.map(|s| s.as_str())).map_err(err)?;
            db::set_playlist_background(&conn, id, map_backdrop(&p.background).as_ref()).map_err(err)?;
            db::set_playlist_hidden(&conn, id, p.hidden).map_err(err)?;
            if let (Some(source), Some(source_id)) = (&p.source, &p.source_id) {
                db::set_playlist_source(&conn, id, source, source_id).map_err(err)?;
            }
        }
    }

    let mut settings = b.settings;
    settings.appearance.background = map_backdrop(&settings.appearance.background);

    Ok(Restored {
        summary: RestoreSummary {
            playlists: b.playlists.len(),
            presets: b.eq_presets.len(),
            artwork: cover_map.len(),
            media: media_map.len(),
            skipped_media,
            folders_added,
            folders_missing,
            relocated,
            unresolved,
            ui: b.ui,
            settings,
        },
        eq_presets: b.eq_presets,
    })
}

/// Maps file paths from the old PC to this one.
struct PathMapper {
    old_roots: Vec<PathBuf>,
    new_roots: Vec<PathBuf>,
    /// Old folder -> folder on this PC (None: not found).
    dirs: HashMap<PathBuf, Option<PathBuf>>,
}

impl PathMapper {
    fn new(old_roots: &[String], new_roots: &[String]) -> Self {
        Self {
            old_roots: old_roots.iter().map(PathBuf::from).collect(),
            new_roots: new_roots.iter().map(PathBuf::from).collect(),
            dirs: HashMap::new(),
        }
    }

    /// Lookups are cached per folder, so a backup full of paths on a drive that isn't here (or a
    /// disconnected network share) costs one check per folder, not per file.
    fn map(&mut self, old: &str) -> Option<String> {
        let p = Path::new(old);
        let (dir, name) = (p.parent()?, p.file_name()?);
        let new_dir = match self.dirs.get(dir) {
            Some(d) => d.clone(),
            None => {
                let d = self.find_dir(dir);
                self.dirs.insert(dir.to_path_buf(), d.clone());
                d
            }
        };
        let f = new_dir?.join(name);
        f.is_file().then(|| f.to_string_lossy().to_string())
    }

    fn find_dir(&self, old_dir: &Path) -> Option<PathBuf> {
        if old_dir.is_dir() {
            return Some(old_dir.to_path_buf());
        }
        // Folders below the old library root ("Artist\Album"); otherwise everything after the drive.
        let (rel, tail_len): (Vec<_>, usize) = match self.old_roots.iter().find(|r| old_dir.starts_with(r)) {
            Some(r) => {
                let tail: Vec<_> = old_dir.strip_prefix(r).ok()?.components().collect();
                let n = tail.len();
                // Include the old root's own name so "D:\Music" can match "E:\Stuff\Music".
                (r.file_name().map(std::path::Component::Normal).into_iter().chain(tail).collect(), n)
            }
            None => {
                let all: Vec<_> = old_dir.components().filter(|c| matches!(c, std::path::Component::Normal(_))).collect();
                let n = all.len();
                (all, n)
            }
        };
        // Longest trailing match first. Require two folder names when there are that many, so a
        // generic folder like "Disc 1" can't match the wrong album.
        let min = tail_len.min(2).max(1);
        for k in (min..=rel.len()).rev() {
            for root in &self.new_roots {
                let cand = rel[rel.len() - k..].iter().fold(root.clone(), |acc, c| acc.join(c));
                if cand.is_dir() {
                    return Some(cand);
                }
            }
        }
        // Files that sat directly in the old library root -> the (single) library root here.
        if self.old_roots.iter().any(|r| r == old_dir) && self.new_roots.len() == 1 {
            return Some(self.new_roots[0].clone());
        }
        None
    }
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remaps_moved_library() {
        let base = std::env::temp_dir().join(format!("resonance-backup-test-{}", std::process::id()));
        let album = base.join("Stuff").join("Music").join("Artist").join("Album");
        std::fs::create_dir_all(&album).unwrap();
        std::fs::write(album.join("01 Song.flac"), b"x").unwrap();
        std::fs::create_dir_all(base.join("Stuff").join("Music").join("Other").join("Disc 1")).unwrap();
        std::fs::write(base.join("Stuff").join("Music").join("loose.mp3"), b"x").unwrap();
        let new_music = base.join("Stuff").join("Music").to_string_lossy().to_string();
        let new_stuff = base.join("Stuff").to_string_lossy().to_string();
        let expected = album.join("01 Song.flac").to_string_lossy().to_string();
        // The "old PC" lives in a folder that doesn't exist (no real drive letters: they may be network shares).
        let old = |rest: &str| base.join("OldPC").join(rest).to_string_lossy().to_string();
        let old_root = old("Music");

        // Same root name, different location.
        let mut m = PathMapper::new(&[old_root.clone()], &[new_music.clone()]);
        assert_eq!(m.map(&old(r"Music\Artist\Album\01 Song.flac")), Some(expected.clone()));
        // An existing path stays as it is.
        assert_eq!(m.map(&expected), Some(expected.clone()));
        // Library root chosen one level higher on the new PC.
        let mut m = PathMapper::new(&[old_root.clone()], &[new_stuff]);
        assert_eq!(m.map(&old(r"Music\Artist\Album\01 Song.flac")), Some(expected.clone()));
        // File that wasn't under any backed-up root.
        let mut m = PathMapper::new(&[], &[new_music.clone()]);
        assert_eq!(m.map(&old(r"Users\me\Music\Artist\Album\01 Song.flac")), Some(expected));
        // Files directly in the old root.
        let mut m = PathMapper::new(&[old_root.clone()], &[new_music.clone()]);
        assert!(m.map(&old(r"Music\loose.mp3")).is_some());
        // A generic folder name alone must not match another album's "Disc 1".
        let mut m = PathMapper::new(&[old_root], &[new_music]);
        assert_eq!(m.map(&old(r"Music\Someone\Else\Disc 1\x.flac")), None);
        // Missing entirely.
        assert_eq!(m.map(&old(r"Music\Nope\Never\a.flac")), None);

        let _ = std::fs::remove_dir_all(&base);
    }

    const MB: u64 = 1024 * 1024;

    #[test]
    fn media_size_caps() {
        let sizes: Vec<(String, u64)> = vec![
            ("small".into(), MB),
            ("huge".into(), 26 * MB),
            ("edge".into(), 25 * MB),
            ("a".into(), 20 * MB),
            ("b".into(), 20 * MB),
        ];
        let (inc, skip) = plan_media(&sizes, 25 * MB, 200 * MB);
        assert_eq!(inc, vec!["small", "a", "b", "edge"]);
        assert_eq!(skip.len(), 1);
        assert_eq!(skip[0].0, "huge");
        assert!(skip[0].1.contains("25 MB"));

        // The total cap: ten 24 MB files, eight fit in 200 MB.
        let many: Vec<(String, u64)> = (0..10).map(|i| (format!("m{i}"), 24 * MB)).collect();
        let (inc, skip) = plan_media(&many, 25 * MB, 200 * MB);
        assert_eq!((inc.len(), skip.len()), (8, 2));
        assert!(skip.iter().all(|(_, why)| why.contains("200 MB")));
        // Smaller files first, so the most items fit.
        let mixed = vec![("big".to_string(), 24 * MB), ("tiny".to_string(), MB)];
        assert_eq!(plan_media(&mixed, 25 * MB, 24 * MB).0, vec!["tiny"]);
    }

    fn temp_library(dir: &Path) -> std::sync::Arc<Library> {
        std::fs::create_dir_all(dir).unwrap();
        Library::open(&dir.join("library.db"), dir.join("covers"), dir.join("media"), std::sync::Arc::new(|_| {})).unwrap()
    }

    fn jpeg(rgb: [u8; 3]) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(32, 32, image::Rgb(rgb));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(img).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg).unwrap();
        out
    }

    fn add_media(lib: &Library, data: &[u8], ext: &str, kind: MediaKind, poster: &[u8]) -> String {
        let id = lib.media.put(data, ext).unwrap();
        let poster_id = lib.covers.store(poster).unwrap();
        let m = Media {
            id: id.clone(),
            kind,
            mime: media::mime_for_ext(ext).unwrap().into(),
            width: 640,
            height: 360,
            duration_ms: (kind == MediaKind::Video).then_some(5000),
            bytes: data.len() as u64,
            poster_id: Some(poster_id),
        };
        db::put_media(&lib.db.lock(), &m).unwrap();
        id
    }

    #[test]
    fn media_round_trips_and_oversized_files_are_listed() {
        let base = std::env::temp_dir().join(format!("resonance-backup-media-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let old = temp_library(&base.join("old"));

        // A playlist with a GIF cover and a video background, a song with a looping cover, and an
        // app background too large for a backup.
        let gif = add_media(&old, b"GIF89a pretend animation", "gif", MediaKind::Gif, &jpeg([200, 0, 0]));
        let video = add_media(&old, b"pretend mp4 background", "mp4", MediaKind::Video, &jpeg([0, 200, 0]));
        let song_loop = add_media(&old, b"pretend mp4 cover loop", "mp4", MediaKind::Video, &jpeg([0, 0, 200]));
        let mut big = b"pretend huge video".to_vec();
        big.resize((MEDIA_ITEM_MAX + 1) as usize, 0);
        let skin = add_media(&old, &big, "mp4", MediaKind::Video, &jpeg([9, 9, 9]));
        let gif_poster = db::media_by_id(&old.db.lock(), &gif).unwrap().unwrap().poster_id.unwrap();
        let song_poster = db::media_by_id(&old.db.lock(), &song_loop).unwrap().unwrap().poster_id.unwrap();
        {
            let conn = old.db.lock();
            let p = db::create_playlist(&conn, "Night drive").unwrap();
            db::set_playlist_cover(&conn, p, Some(&gif_poster), Some(&gif)).unwrap();
            let bg = Backdrop { media_id: video.clone(), fit: "cover".into(), dim: Some(0.6), blur: 8.0 };
            db::set_playlist_background(&conn, p, Some(&bg)).unwrap();
        }
        let song = r"C:\Music\song.flac".to_string();
        db::put_track_art(
            &mut old.db.lock(),
            &[TrackArt { path: song.clone(), cover_id: Some(song_poster), motion_id: Some(song_loop.clone()), hidden: false }],
        )
        .unwrap();
        let mut settings = Settings::default();
        settings.appearance.background = Some(Backdrop { media_id: skin.clone(), ..Backdrop::default() });

        let file = base.join("test.resonance-backup");
        let summary = export(&old, settings, Vec::new(), serde_json::Value::Null, &file).unwrap();
        assert_eq!((summary.media, summary.media_skipped), (3, 1));

        let new = temp_library(&base.join("new"));
        let restored = restore(&new, read(&file).unwrap(), |_| Ok(())).unwrap().summary;
        assert_eq!(restored.media, 3);
        assert_eq!(restored.skipped_media.len(), 1);
        assert!(restored.skipped_media[0].starts_with("App background"), "{:?}", restored.skipped_media);
        assert!(restored.skipped_media[0].contains("25 MB"));
        assert_eq!(restored.settings.appearance.background, None, "the skipped background isn't restored");

        let conn = new.db.lock();
        let p = db::playlists(&conn).unwrap().remove(0);
        assert_eq!(p.name, "Night drive");
        let motion = p.motion_id.expect("GIF cover restored");
        assert_eq!(db::media_by_id(&conn, &motion).unwrap().unwrap().kind, MediaKind::Gif);
        assert!(new.media.file(&motion).is_some());
        assert!(p.cover_id.is_some_and(|c| new.covers.file(&c, false).is_some()), "poster restored as the cover");
        let bg = p.background.expect("background restored");
        assert_eq!((bg.dim, bg.blur), (Some(0.6), 8.0));
        let bg_row = db::media_by_id(&conn, &bg.media_id).unwrap().unwrap();
        assert!(bg_row.poster_id.is_some_and(|c| new.covers.file(&c, false).is_some()), "background poster restored");
        let art = db::track_art(&conn).unwrap();
        assert_eq!(art[0].motion_id.as_deref(), Some(song_loop.as_str()), "same bytes, same id");
        drop(conn);
        let _ = std::fs::remove_dir_all(&base);
    }
}
