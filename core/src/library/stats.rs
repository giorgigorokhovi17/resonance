//! Listening history and the stats page ("Wrapped"). A play is recorded once a song has been
//! heard for 30 seconds, or for half of it when it's shorter than a minute. Everything stays in
//! the local database.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use crossbeam_channel::{unbounded, Sender};
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use serde::Serialize;

use super::db;
use super::model::Track;
use super::Library;

#[derive(Clone, Debug, PartialEq)]
pub struct Play {
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    /// Unix seconds when the song started.
    pub played_at: i64,
    /// Seconds actually heard.
    pub listened: f64,
}

pub fn record(conn: &Connection, p: &Play) -> Result<()> {
    conn.execute(
        "INSERT INTO plays(path, title, artist, album, album_artist, played_at, listened)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![p.path, p.title, p.artist, p.album, p.album_artist, p.played_at, p.listened],
    )?;
    Ok(())
}

/// Whether `listened` seconds of a song `duration` seconds long count as a play.
pub fn counts(listened: f64, duration: f64) -> bool {
    let need = if duration > 0.0 && duration < 60.0 { duration / 2.0 } else { 30.0 };
    listened >= need
}

struct Current {
    uid: u64,
    play: Play,
    duration: f64,
    /// When the last status update arrived while playing.
    last: Option<Instant>,
}

/// Follows the player's status updates and records each song heard long enough to count.
pub struct PlayTracker {
    current: Mutex<Option<Current>>,
    tx: Sender<Play>,
}

impl PlayTracker {
    /// Plays are written on their own thread, never on the audio thread that reports status.
    pub fn new(library: Arc<Library>) -> Arc<Self> {
        let (tx, rx) = unbounded::<Play>();
        let _ = std::thread::Builder::new().name("play-history".into()).spawn(move || {
            for p in rx {
                if let Err(e) = record(&library.db.lock(), &p) {
                    log::warn!("could not record a play: {e}");
                }
            }
        });
        Arc::new(Self { current: Mutex::new(None), tx })
    }

    /// One status update: which queue item is playing (if any) and whether it's playing now.
    pub fn update(&self, uid: Option<u64>, track: Option<&Track>, playing: bool) {
        self.update_at(uid, track, playing, Instant::now(), db::now());
    }

    fn update_at(&self, uid: Option<u64>, track: Option<&Track>, playing: bool, now: Instant, unix: i64) {
        let mut cur = self.current.lock();
        if cur.as_ref().map(|c| c.uid) != uid {
            if let Some(done) = cur.take() {
                self.send(done);
            }
            if let (Some(uid), Some(t)) = (uid, track) {
                *cur = Some(Current {
                    uid,
                    play: Play {
                        path: t.path.clone(),
                        title: t.title.clone(),
                        artist: t.artist.clone(),
                        album: t.album.clone(),
                        album_artist: t.album_artist.clone(),
                        played_at: unix,
                        listened: 0.0,
                    },
                    duration: t.duration,
                    last: None,
                });
            }
        }
        if let Some(c) = cur.as_mut() {
            if playing {
                if let Some(last) = c.last {
                    // Updates come every 200 ms while playing; a longer gap means it wasn't.
                    c.play.listened += now.saturating_duration_since(last).as_secs_f64().min(1.0);
                }
                c.last = Some(now);
            } else {
                c.last = None;
            }
        }
    }

    fn send(&self, c: Current) {
        if counts(c.play.listened, c.duration) {
            let _ = self.tx.send(c.play);
        }
    }

    /// Records the song playing now, if it counts (when the app quits).
    pub fn finish(&self, library: &Library) {
        if let Some(c) = self.current.lock().take() {
            if counts(c.play.listened, c.duration) {
                if let Err(e) = record(&library.db.lock(), &c.play) {
                    log::warn!("could not record a play: {e}");
                }
            }
        }
    }
}

// ---- the stats page -------------------------------------------------------------------------------

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TopSong {
    pub path: String,
    pub title: String,
    pub artist: String,
    pub plays: u32,
    pub seconds: f64,
    /// The song in the library now (for artwork and playing it), if it's still there.
    pub track: Option<Track>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TopArtist {
    pub name: String,
    pub plays: u32,
    pub seconds: f64,
    pub songs: u32,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TopAlbum {
    pub album: String,
    pub artist: String,
    pub plays: u32,
    pub seconds: f64,
    /// One of its songs in the library now, for artwork.
    pub track: Option<Track>,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub plays: u32,
    pub seconds: f64,
    pub songs: u32,
    pub artists: u32,
    pub albums: u32,
    pub top_songs: Vec<TopSong>,
    pub top_artists: Vec<TopArtist>,
    pub top_albums: Vec<TopAlbum>,
    /// Seconds listened in each hour of the day (local time).
    pub by_hour: Vec<f64>,
    /// Seconds listened on each weekday, Monday first.
    pub by_weekday: Vec<f64>,
    /// The day with the most listening: (unix seconds at its local midnight, seconds listened).
    pub top_day: Option<(i64, f64)>,
    /// Most days in a row with at least one play.
    pub longest_streak: u32,
    /// Unix seconds of the first play ever recorded (any period).
    pub first_play: Option<i64>,
}

fn artist_of<'a>(artist: &'a str, album_artist: &'a str) -> &'a str {
    if artist.trim().is_empty() {
        album_artist
    } else {
        artist
    }
}

/// Stats for plays started in `from..to` (unix seconds). `tz_offset_min` is the local time's
/// offset from UTC, used for hours, weekdays and days.
pub fn stats(conn: &Connection, from: i64, to: i64, tz_offset_min: i64, limit: usize) -> Result<Stats> {
    let mut stmt = conn.prepare(
        "SELECT path, title, artist, album, album_artist, played_at, listened FROM plays
         WHERE played_at >= ?1 AND played_at < ?2 ORDER BY played_at",
    )?;
    let plays = stmt
        .query_map(params![from, to], |r| {
            Ok(Play {
                path: r.get(0)?,
                title: r.get(1)?,
                artist: r.get(2)?,
                album: r.get(3)?,
                album_artist: r.get(4)?,
                played_at: r.get(5)?,
                listened: r.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let first_play: Option<i64> = conn.query_row("SELECT MIN(played_at) FROM plays", [], |r| r.get(0))?;
    let mut s = summarize(&plays, tz_offset_min, limit);
    s.first_play = first_play;
    for song in &mut s.top_songs {
        song.track = db::track_by_path(conn, &song.path)?;
    }
    // An album's artwork: the first of its played songs still in the library.
    let mut album_paths: HashMap<(String, String), Vec<&str>> = HashMap::new();
    for p in &plays {
        let key = (p.album.to_lowercase(), artist_of(&p.album_artist, &p.artist).to_lowercase());
        let paths = album_paths.entry(key).or_default();
        if !paths.contains(&p.path.as_str()) {
            paths.push(&p.path);
        }
    }
    for album in &mut s.top_albums {
        let key = (album.album.to_lowercase(), album.artist.to_lowercase());
        for path in album_paths.get(&key).map(|v| v.as_slice()).unwrap_or_default().iter().take(5) {
            if let Some(t) = db::track_by_path(conn, path)? {
                album.track = Some(t);
                break;
            }
        }
    }
    Ok(s)
}

/// The numbers behind the stats page, from plays sorted by time.
fn summarize(plays: &[Play], tz_offset_min: i64, limit: usize) -> Stats {
    let offset = tz_offset_min * 60;
    let mut s = Stats { by_hour: vec![0.0; 24], by_weekday: vec![0.0; 7], ..Default::default() };
    // Keyed case-insensitively; the first spelling seen is shown.
    let mut songs: HashMap<String, TopSong> = HashMap::new();
    let mut artists: HashMap<String, (TopArtist, std::collections::HashSet<String>)> = HashMap::new();
    let mut albums: HashMap<(String, String), TopAlbum> = HashMap::new();
    let mut days: HashMap<i64, f64> = HashMap::new();
    for p in plays {
        s.plays += 1;
        s.seconds += p.listened;
        let artist = artist_of(&p.artist, &p.album_artist);
        let song = songs.entry(p.path.to_lowercase()).or_insert_with(|| TopSong {
            path: p.path.clone(),
            title: p.title.clone(),
            artist: artist.to_string(),
            plays: 0,
            seconds: 0.0,
            track: None,
        });
        song.plays += 1;
        song.seconds += p.listened;
        if !artist.trim().is_empty() {
            let (a, set) = artists.entry(artist.to_lowercase()).or_insert_with(|| {
                (TopArtist { name: artist.to_string(), plays: 0, seconds: 0.0, songs: 0 }, Default::default())
            });
            a.plays += 1;
            a.seconds += p.listened;
            set.insert(p.path.to_lowercase());
            a.songs = set.len() as u32;
        }
        if !p.album.trim().is_empty() {
            let album_artist = artist_of(&p.album_artist, &p.artist);
            let al = albums.entry((p.album.to_lowercase(), album_artist.to_lowercase())).or_insert_with(|| TopAlbum {
                album: p.album.clone(),
                artist: album_artist.to_string(),
                plays: 0,
                seconds: 0.0,
                track: None,
            });
            al.plays += 1;
            al.seconds += p.listened;
        }
        let local = p.played_at + offset;
        let day = local.div_euclid(86_400);
        s.by_hour[(local.rem_euclid(86_400) / 3600) as usize] += p.listened;
        // 1970-01-01 was a Thursday.
        s.by_weekday[((day + 3).rem_euclid(7)) as usize] += p.listened;
        *days.entry(day).or_default() += p.listened;
    }
    s.songs = songs.len() as u32;
    s.artists = artists.len() as u32;
    s.albums = albums.len() as u32;

    let mut top_songs: Vec<TopSong> = songs.into_values().collect();
    top_songs.sort_by(|a, b| b.plays.cmp(&a.plays).then(b.seconds.total_cmp(&a.seconds)).then(a.title.cmp(&b.title)));
    top_songs.truncate(limit);
    s.top_songs = top_songs;
    let mut top_artists: Vec<TopArtist> = artists.into_values().map(|(a, _)| a).collect();
    top_artists.sort_by(|a, b| b.seconds.total_cmp(&a.seconds).then(b.plays.cmp(&a.plays)).then(a.name.cmp(&b.name)));
    top_artists.truncate(limit);
    s.top_artists = top_artists;
    let mut top_albums: Vec<TopAlbum> = albums.into_values().collect();
    top_albums.sort_by(|a, b| b.seconds.total_cmp(&a.seconds).then(b.plays.cmp(&a.plays)).then(a.album.cmp(&b.album)));
    top_albums.truncate(limit);
    s.top_albums = top_albums;

    s.top_day = days
        .iter()
        .max_by(|a, b| a.1.total_cmp(b.1).then(b.0.cmp(a.0)))
        .map(|(d, secs)| (d * 86_400 - offset, *secs));
    let mut sorted: Vec<i64> = days.keys().copied().collect();
    sorted.sort_unstable();
    let (mut best, mut run) = (0u32, 0u32);
    for (i, d) in sorted.iter().enumerate() {
        run = if i > 0 && sorted[i - 1] == d - 1 { run + 1 } else { 1 };
        best = best.max(run);
    }
    s.longest_streak = best;
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn play(path: &str, artist: &str, album: &str, at: i64, listened: f64) -> Play {
        Play {
            path: path.into(),
            title: path.trim_end_matches(".flac").into(),
            artist: artist.into(),
            album: album.into(),
            album_artist: String::new(),
            played_at: at,
            listened,
        }
    }

    #[test]
    fn a_play_needs_30_seconds_or_half_a_short_song() {
        assert!(!counts(29.9, 240.0));
        assert!(counts(30.0, 240.0));
        assert!(counts(20.0, 40.0));
        assert!(!counts(19.0, 40.0));
        assert!(counts(30.0, 0.0), "unknown length: 30 s");
    }

    #[test]
    fn tracker_counts_only_time_spent_playing() {
        let (tx, rx) = unbounded();
        let tracker = PlayTracker { current: Mutex::new(None), tx };
        let t = Track { path: "a.flac".into(), title: "A".into(), duration: 200.0, ..Default::default() };
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        // 25 s of 200 ms updates, a pause, then 10 s more.
        let mut ms = 0;
        while ms <= 25_000 {
            tracker.update_at(Some(1), Some(&t), true, at(ms), 1000);
            ms += 200;
        }
        tracker.update_at(Some(1), Some(&t), false, at(26_000), 1000);
        tracker.update_at(Some(1), Some(&t), false, at(300_000), 1000);
        let mut ms = 400_000;
        while ms <= 410_000 {
            tracker.update_at(Some(1), Some(&t), true, at(ms), 1000);
            ms += 200;
        }
        // A skip after 2 s doesn't count; the first song (35 s) does.
        let b = Track { path: "b.flac".into(), title: "B".into(), duration: 200.0, ..Default::default() };
        tracker.update_at(Some(2), Some(&b), true, at(410_200), 1400);
        tracker.update_at(Some(2), Some(&b), true, at(412_200), 1400);
        tracker.update_at(None, None, false, at(412_400), 1400);
        let got: Vec<Play> = rx.try_iter().collect();
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].path.as_str(), got[0].played_at), ("a.flac", 1000));
        assert!((got[0].listened - 35.0).abs() < 0.01, "{}", got[0].listened);
    }

    #[test]
    fn summary_ranks_and_buckets() {
        // 2026-01-05 is a Monday; 10:00 UTC is 14:00 at UTC+4.
        let monday = 1_767_607_200;
        let plays = vec![
            play("one.flac", "Alpha", "First", monday, 100.0),
            play("one.flac", "alpha", "First", monday + 3600, 100.0),
            play("two.flac", "Beta", "Second", monday + 86_400, 300.0),
            play("three.flac", "Alpha", "First", monday + 3 * 86_400, 50.0),
        ];
        let s = summarize(&plays, 240, 10);
        assert_eq!((s.plays, s.songs, s.artists, s.albums), (4, 3, 2, 2));
        assert_eq!(s.seconds, 550.0);
        assert_eq!(s.top_songs[0].path, "one.flac");
        assert_eq!(s.top_songs[0].plays, 2);
        assert_eq!((s.top_artists[0].name.as_str(), s.top_artists[0].songs), ("Beta", 1));
        assert_eq!((s.top_artists[1].name.as_str(), s.top_artists[1].seconds), ("Alpha", 250.0));
        assert_eq!(s.top_albums[0].album, "Second");
        assert_eq!(s.by_hour[14], 450.0, "every day at 14:00 but one");
        assert_eq!(s.by_hour[15], 100.0);
        assert_eq!(s.by_weekday[0], 200.0, "Monday");
        assert_eq!(s.by_weekday[1], 300.0, "Tuesday");
        assert_eq!(s.longest_streak, 2);
        let (day, secs) = s.top_day.unwrap();
        assert_eq!(secs, 300.0);
        assert_eq!((day + 240 * 60) % 86_400, 0, "local midnight");
        assert_eq!(summarize(&[], 0, 10), Stats { by_hour: vec![0.0; 24], by_weekday: vec![0.0; 7], ..Default::default() });
    }

    #[test]
    fn stats_read_from_the_database() {
        let conn = db::open(std::path::Path::new(":memory:")).unwrap();
        record(&conn, &play("a.flac", "Alpha", "First", 100, 40.0)).unwrap();
        record(&conn, &play("b.flac", "Beta", "", 5000, 40.0)).unwrap();
        let s = stats(&conn, 0, 1000, 0, 10).unwrap();
        assert_eq!((s.plays, s.first_play), (1, Some(100)));
        assert_eq!(s.top_songs[0].track, None, "not in the library");
        assert_eq!(stats(&conn, 0, i64::MAX, 0, 10).unwrap().plays, 2);
    }
}
