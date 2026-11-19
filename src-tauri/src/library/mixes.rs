//! Automatic mixes built from the local listening history (`plays`) and the library (`tracks`).
//! They update on the fly and are not saved as playlists unless the user saves them.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;

use super::db;
use super::model::Track;

const DAY: i64 = 86_400;
const MIX_LIMIT: usize = 30;
const ON_REPEAT_DAYS: i64 = 30;
const FORGOTTEN_MIN_PLAYS: i64 = 5;
const FORGOTTEN_DAYS: i64 = 60;
const FRESH_DAYS: i64 = 30;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SmartMix {
    pub id: String,
    pub title: String,
    pub description: String,
    pub tracks: Vec<Track>,
}

/// Builds the three smart mixes as of now, using `tz_offset_min` (local offset from UTC) for day
/// boundaries. Songs no longer in the library are left out.
pub fn smart_mixes(conn: &Connection, tz_offset_min: i64) -> Result<Vec<SmartMix>> {
    smart_mixes_at(conn, db::now(), tz_offset_min)
}

fn smart_mixes_at(conn: &Connection, now: i64, tz_offset_min: i64) -> Result<Vec<SmartMix>> {
    let offset = tz_offset_min.clamp(-14 * 60, 14 * 60) * 60;
    let local_today = (now + offset).div_euclid(DAY);
    let days_ago = |days: i64| (local_today - days) * DAY - offset;

    Ok(vec![
        SmartMix {
            id: "on-repeat".into(),
            title: "On repeat".into(),
            description: "Played most in the last 30 days".into(),
            tracks: on_repeat(conn, days_ago(ON_REPEAT_DAYS))?,
        },
        SmartMix {
            id: "forgotten-favourites".into(),
            title: "Forgotten favourites".into(),
            description: "Played often, but not in the last 60 days".into(),
            tracks: forgotten_favourites(conn, days_ago(FORGOTTEN_DAYS))?,
        },
        SmartMix {
            id: "fresh-in-library".into(),
            title: "Fresh in your library".into(),
            description: "Added in the last 30 days, not played yet".into(),
            tracks: fresh_in_library(conn, days_ago(FRESH_DAYS))?,
        },
    ])
}

fn resolve_paths(conn: &Connection, paths: &[String]) -> Result<Vec<Track>> {
    let mut out = Vec::with_capacity(paths.len());
    for path in paths {
        if let Some(track) = db::track_by_path(conn, path)? {
            out.push(track);
        }
    }
    Ok(out)
}

/// The 30 library songs played most since `since`.
fn on_repeat(conn: &Connection, since: i64) -> Result<Vec<Track>> {
    let mut stmt = conn.prepare(
        "SELECT p.path
         FROM plays p
         INNER JOIN tracks t ON t.path = p.path
         WHERE p.played_at >= ?1
         GROUP BY p.path
         ORDER BY COUNT(*) DESC, SUM(p.listened) DESC, MAX(p.played_at) DESC, p.path ASC
         LIMIT ?2",
    )?;
    let paths = stmt
        .query_map(params![since, MIX_LIMIT as i64], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    resolve_paths(conn, &paths)
}

/// Library songs played at least 5 times in total, but not since `before`, most played first.
fn forgotten_favourites(conn: &Connection, before: i64) -> Result<Vec<Track>> {
    let mut stmt = conn.prepare(
        "SELECT p.path
         FROM plays p
         INNER JOIN tracks t ON t.path = p.path
         GROUP BY p.path
         HAVING COUNT(*) >= ?1 AND MAX(p.played_at) < ?2
         ORDER BY COUNT(*) DESC, SUM(p.listened) DESC, MAX(p.played_at) DESC, p.path ASC
         LIMIT ?3",
    )?;
    let paths = stmt
        .query_map(params![FORGOTTEN_MIN_PLAYS, before, MIX_LIMIT as i64], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    resolve_paths(conn, &paths)
}

/// Library songs added since `since` that have never been played, newest first.
fn fresh_in_library(conn: &Connection, since: i64) -> Result<Vec<Track>> {
    let mut stmt = conn.prepare(
        "SELECT t.path
         FROM tracks t
         WHERE t.added_at >= ?1
           AND NOT EXISTS (SELECT 1 FROM plays p WHERE p.path = t.path)
         ORDER BY t.added_at DESC, t.id DESC
         LIMIT ?2",
    )?;
    let paths = stmt
        .query_map(params![since, MIX_LIMIT as i64], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    resolve_paths(conn, &paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::stats::{record, Play};

    fn mem_db() -> (Connection, i64) {
        let conn = db::open(std::path::Path::new(":memory:")).unwrap();
        let folder = db::add_folder(&conn, "C:\\Music").unwrap();
        (conn, folder)
    }

    fn add_track(conn: &Connection, folder_id: i64, path: &str, added_at: i64) {
        conn.execute(
            "INSERT INTO tracks(folder_id, path, dir, title, artist, album, album_artist, genre,
                duration, codec, file_size, mtime, added_at)
             VALUES(?1, ?2, 'C:\\Music', ?3, 'Artist', 'Album', '', '', 180.0, 'FLAC', 1000, 1, ?4)",
            params![folder_id, path, path.trim_end_matches(".flac"), added_at],
        )
        .unwrap();
    }

    fn add_play(conn: &Connection, path: &str, played_at: i64, listened: f64) {
        record(
            conn,
            &Play {
                path: path.into(),
                title: path.trim_end_matches(".flac").into(),
                artist: "Artist".into(),
                album: "Album".into(),
                album_artist: String::new(),
                played_at,
                listened,
            },
        )
        .unwrap();
    }

    fn paths_of(mix: &SmartMix) -> Vec<&str> {
        mix.tracks.iter().map(|t| t.path.as_str()).collect()
    }

    #[test]
    fn on_repeat_ranks_recent_plays_and_skips_removed_tracks() {
        let (conn, folder) = mem_db();
        let now = 100 * DAY;
        add_track(&conn, folder, "top.flac", 10 * DAY);
        add_track(&conn, folder, "second.flac", 10 * DAY);
        add_track(&conn, folder, "old.flac", 10 * DAY);

        // 3 recent plays for top, 2 for second, 5 for a deleted song, 10 old plays for old.
        for i in 0..3 {
            add_play(&conn, "top.flac", now - (i + 1) * DAY, 120.0);
        }
        for i in 0..2 {
            add_play(&conn, "second.flac", now - (i + 1) * DAY, 120.0);
        }
        for i in 0..5 {
            add_play(&conn, "deleted.flac", now - (i + 1) * DAY, 120.0);
        }
        for i in 0..10 {
            add_play(&conn, "old.flac", now - (35 + i) * DAY, 120.0);
        }

        let mixes = smart_mixes_at(&conn, now, 0).unwrap();
        let repeat = mixes.iter().find(|m| m.id == "on-repeat").unwrap();
        assert_eq!(paths_of(repeat), vec!["top.flac", "second.flac"]);
    }

    #[test]
    fn forgotten_favourites_needs_five_plays_and_none_in_last_60_days() {
        let (conn, folder) = mem_db();
        let now = 200 * DAY;
        add_track(&conn, folder, "most.flac", 1 * DAY);
        add_track(&conn, folder, "five.flac", 1 * DAY);
        add_track(&conn, folder, "four.flac", 1 * DAY);
        add_track(&conn, folder, "recent.flac", 1 * DAY);

        // 8 old plays -> first.
        for i in 0..8 {
            add_play(&conn, "most.flac", now - (70 + i) * DAY, 100.0);
        }
        // 5 old plays -> second.
        for i in 0..5 {
            add_play(&conn, "five.flac", now - (65 + i) * DAY, 100.0);
        }
        // Only 4 old plays -> excluded.
        for i in 0..4 {
            add_play(&conn, "four.flac", now - (70 + i) * DAY, 100.0);
        }
        // 6 old plays + 1 recent play -> excluded.
        for i in 0..6 {
            add_play(&conn, "recent.flac", now - (80 + i) * DAY, 100.0);
        }
        add_play(&conn, "recent.flac", now - 10 * DAY, 100.0);
        // 10 old plays for a song no longer in the library -> excluded.
        for i in 0..10 {
            add_play(&conn, "gone.flac", now - (90 + i) * DAY, 100.0);
        }

        let mixes = smart_mixes_at(&conn, now, 0).unwrap();
        let forgotten = mixes.iter().find(|m| m.id == "forgotten-favourites").unwrap();
        assert_eq!(paths_of(forgotten), vec!["most.flac", "five.flac"]);
    }

    #[test]
    fn fresh_in_library_lists_recent_unplayed_newest_first() {
        let (conn, folder) = mem_db();
        let now = 100 * DAY;
        add_track(&conn, folder, "older_fresh.flac", now - 20 * DAY);
        add_track(&conn, folder, "newest.flac", now - 2 * DAY);
        add_track(&conn, folder, "played_fresh.flac", now - 1 * DAY);
        add_track(&conn, folder, "stale_unplayed.flac", now - 40 * DAY);

        add_play(&conn, "played_fresh.flac", now - 100, 60.0);

        let mixes = smart_mixes_at(&conn, now, 0).unwrap();
        let fresh = mixes.iter().find(|m| m.id == "fresh-in-library").unwrap();
        assert_eq!(paths_of(fresh), vec!["newest.flac", "older_fresh.flac"]);
    }

    #[test]
    fn each_mix_caps_at_thirty_songs() {
        let (conn, folder) = mem_db();
        let now = 200 * DAY;
        for i in 0..35 {
            let p_rep = format!("rep_{i:02}.flac");
            add_track(&conn, folder, &p_rep, 1 * DAY);
            add_play(&conn, &p_rep, now - DAY, 100.0);

            let p_for = format!("for_{i:02}.flac");
            add_track(&conn, folder, &p_for, 1 * DAY);
            for k in 0..5 {
                add_play(&conn, &p_for, now - (70 + k) * DAY, 100.0);
            }

            let p_fresh = format!("fresh_{i:02}.flac");
            add_track(&conn, folder, &p_fresh, now - i * 3600);
        }

        let mixes = smart_mixes_at(&conn, now, 0).unwrap();
        assert_eq!(mixes.len(), 3);
        for mix in &mixes {
            assert_eq!(mix.tracks.len(), 30, "{}", mix.id);
        }
    }

    #[test]
    fn timezone_offset_shifts_day_boundary() {
        let (conn, folder) = mem_db();
        // 2026-01-31 01:00 UTC: at UTC-5 it is still 2026-01-30 20:00, so 30 days ago starts on
        // 2025-12-31 05:00 UTC; at UTC+0 it starts on 2026-01-01 00:00 UTC.
        let now = 1_769_821_200; // 2026-01-31 01:00:00 UTC
        let boundary_track = 1_767_204_000; // 2025-12-31 18:00:00 UTC
        add_track(&conn, folder, "edge.flac", boundary_track);

        let utc = smart_mixes_at(&conn, now, 0).unwrap();
        assert!(utc[2].tracks.is_empty(), "outside 30 days at UTC");

        let utc_minus_5 = smart_mixes_at(&conn, now, -300).unwrap();
        assert_eq!(paths_of(&utc_minus_5[2]), vec!["edge.flac"], "inside 30 days at UTC-5");
    }
}
