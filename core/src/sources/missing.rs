//! Finding songs you don't have on Soulseek, for imported playlists.
//!
//! For each missing reference: search Soulseek (artist + title + version words, then title +
//! version words), judge every file in the answers with the playlist matcher's own normalisation,
//! and keep the acceptable ones, best first. Nothing downloads on its own: the user approves files
//! (owner's decision, 2026-10-20), and a finished download is linked into every playlist that has
//! that reference.
//!
//! A file is acceptable when:
//! - its name (track number stripped), read as "Artist - Title" or as a title under an
//!   "Artist - Album" folder, has the same title and the same version qualifiers as the
//!   reference (so a remix or slowed upload is never taken for the original);
//! - the reference's main artist appears somewhere in its path;
//! - if both sides know the length, they're within 3 s;
//! - it's FLAC, or MP3 at 320 kbps or more (owner's choice: "FLAC, else 320 MP3").
//!
//! Ranking: FLAC over MP3, a confirmed length, a free upload slot, then upload speed.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::soulseek::model::{format_of, split_remote, FileAttrs, RawResponse};
use super::soulseek::{ConnState, Soulseek};
use super::youtube::YouTube;
use crate::events::Events;
use crate::library::interchange::TrackRef;
use crate::library::matcher::normalize::{key, parse_title, readings, tokens, Reading};

/// How long each search collects answers.
const SEARCH_WINDOW: Duration = Duration::from_secs(10);
/// Pause between searches, to stay polite to the Soulseek server.
const BETWEEN_SEARCHES: Duration = Duration::from_millis(1500);
const LENGTH_TOLERANCE_S: f64 = 3.0;
const KEEP: usize = 3;

/// One acceptable file.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub username: String,
    pub path: String,
    pub name: String,
    pub folder: String,
    pub size: u64,
    pub format: String,
    #[serde(flatten)]
    pub attrs: FileAttrs,
    pub free_slots: bool,
    /// Bytes per second.
    pub speed: u32,
    pub score: f64,
    /// "FLAC 16/44.1 · length matches · free slot".
    pub why: String,
}

/// Search words: (artist + title + version, title + version), deduplicated, short ones kept.
pub fn queries(r: &TrackRef) -> Vec<String> {
    let rs = readings(&r.title, &r.artists);
    // The title-side reading: "Artist - Title" titles without a separate artist use reading 2.
    let (title_words, version_words, artist_words) = match rs.iter().find(|x| x.from_title).filter(|_| r.artists.is_empty()) {
        Some(b) => {
            let left = parse_title(&r.title).main.split(" - ").next().unwrap_or("").to_string();
            (b.title_tokens.clone(), b.versions.iter().flat_map(|v| tokens(v)).collect::<Vec<_>>(), tokens(&left))
        }
        None => {
            let a = &rs[0];
            let primary = r.artists.first().map(|x| tokens(x.split([',', '&']).next().unwrap_or(x))).unwrap_or_default();
            (a.title_tokens.clone(), a.versions.iter().flat_map(|v| tokens(v)).collect(), primary)
        }
    };
    let join = |parts: &[&Vec<String>]| {
        let mut seen: Vec<String> = Vec::new();
        for w in parts.iter().flat_map(|p| p.iter()) {
            if !seen.contains(w) {
                seen.push(w.clone());
            }
        }
        seen.truncate(9);
        seen.join(" ")
    };
    let mut out = Vec::new();
    let full = join(&[&artist_words, &title_words, &version_words]);
    if full.chars().count() >= 3 {
        out.push(full);
    }
    let short = join(&[&title_words, &version_words]);
    let long_enough = title_words.len() >= 2 || title_words.iter().map(|w| w.chars().count()).sum::<usize>() >= 6;
    if long_enough && !out.contains(&short) {
        out.push(short);
    }
    out
}

/// "01 - Song", "1. Song", "A2 Song", "101-Song" → "Song".
fn strip_track_number(stem: &str) -> &str {
    let s = stem.trim_start();
    let bytes = s.as_bytes();
    let mut i = 0;
    if bytes.first().is_some_and(|b| b.is_ascii_alphabetic()) && bytes.get(1).is_some_and(|b| b.is_ascii_digit()) {
        i = 1; // vinyl sides: A1, B2
    }
    let digits_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() && i - digits_start < 3 {
        i += 1;
    }
    if i == digits_start || i >= bytes.len() {
        return s;
    }
    let rest = &s[i..];
    let trimmed = rest.trim_start_matches([' ', '.', '-', '_', ')']);
    if trimmed.len() < rest.len() && !trimmed.is_empty() {
        trimmed
    } else {
        s
    }
}

/// Quality tier: 2 FLAC, 1 MP3 ≥ 320 kbps, else unacceptable.
pub(super) fn quality(format: &str, attrs: &FileAttrs, size: u64) -> Option<u8> {
    match format {
        "FLAC" => Some(2),
        "MP3" => {
            let kbps = attrs.bitrate.or_else(|| attrs.length.filter(|l| *l > 0).map(|l| (size * 8 / 1000 / l as u64) as u32));
            kbps.filter(|k| *k >= 315).map(|_| 1)
        }
        _ => None,
    }
}

pub fn title_matches(file: &[Reading], wanted: &[Reading]) -> bool {
    file.iter().any(|f| wanted.iter().any(|w| !w.title_key.is_empty() && f.title_key == w.title_key && f.versions == w.versions))
}

/// Readings of a remote file: its name with and without a leading track number (a song can be
/// called "22"), each also under an "Artist - Album" folder.
pub(super) fn file_readings(path: &str) -> Vec<Reading> {
    let (folder, name) = split_remote(path);
    let raw_stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
    let (_, last_folder) = split_remote(folder);
    let folder_artist = last_folder.split(" - ").next().unwrap_or("").to_string();
    let mut file = Vec::new();
    for stem in [raw_stem, strip_track_number(raw_stem)] {
        file.extend(readings(stem, &[]));
        if !folder_artist.is_empty() {
            file.extend(readings(stem, &[folder_artist.clone()]));
        }
    }
    file
}

/// "FLAC 16/44.1", "MP3 320 kbps".
pub(super) fn quality_text(format: &str, attrs: &FileAttrs) -> String {
    match (format, attrs.bit_depth, attrs.sample_rate, attrs.bitrate) {
        ("FLAC", Some(b), Some(sr), _) => format!("FLAC {b}/{}", sr as f64 / 1000.0),
        ("MP3", _, _, Some(k)) => format!("MP3 {k} kbps"),
        (f, ..) => f.to_string(),
    }
}

/// Judge one file against a reference. `None` when it isn't acceptable.
pub fn judge(r: &TrackRef, wanted: &[Reading], user: &RawResponse, path: &str, size: u64, attrs: &FileAttrs) -> Option<Found> {
    let (folder, name) = split_remote(path);
    let format = format_of(name);
    let q = quality(&format, attrs, size)?;
    if !title_matches(&file_readings(path), wanted) {
        return None;
    }
    // The main artist must appear in the path (folder or file name).
    let path_key = key(path);
    let main_artist = wanted.iter().find_map(|w| w.artists.first().cloned());
    if let Some(a) = &main_artist {
        if !path_key.contains(a.as_str()) {
            return None;
        }
    }
    let delta = match (r.duration_ms, attrs.length) {
        (Some(ms), Some(len)) => Some((ms as f64 / 1000.0 - len as f64).abs()),
        _ => None,
    };
    if delta.is_some_and(|d| d > LENGTH_TOLERANCE_S) {
        return None;
    }
    let speed_bonus = (user.speed as f64 / 2_000_000.0).min(1.0) * 5.0;
    let score = q as f64 * 100.0 + if delta.is_some() { 20.0 } else { 0.0 } + if user.free_slots { 10.0 } else { 0.0 } + speed_bonus;
    let mut why = vec![quality_text(&format, attrs)];
    why.push(match delta {
        Some(d) => format!("length matches (Δ {d:.0} s)"),
        None => "length unknown".into(),
    });
    why.push(if user.free_slots { "free slot".into() } else { "in the user's queue".into() });
    Some(Found {
        username: user.username.clone(),
        path: path.to_string(),
        name: name.to_string(),
        folder: folder.to_string(),
        size,
        format,
        attrs: attrs.clone(),
        free_slots: user.free_slots,
        speed: user.speed,
        score,
        why: why.join(" · "),
    })
}

/// Every acceptable file in a set of search answers, best first (at most `KEEP`), plus the
/// smallest acceptable file when it isn't among those (for when space or speed matters more).
pub fn rank(r: &TrackRef, responses: &[RawResponse]) -> Vec<Found> {
    let mut wanted = readings(&r.title, &r.artists);
    // A reference without artists still names one in its "Artist - Title" title: require it.
    if r.artists.is_empty() {
        wanted.retain(|w| w.from_title || wanted_has_no_dash(&r.title));
    }
    let mut out: Vec<Found> = Vec::new();
    for user in responses {
        for f in &user.files {
            let attrs = FileAttrs::from_codes(&f.attrs);
            if let Some(found) = judge(r, &wanted, user, &f.path, f.size, &attrs) {
                if !out.iter().any(|o| o.username == found.username && o.path == found.path) {
                    out.push(found);
                }
            }
        }
    }
    out.sort_by(|a, b| b.score.total_cmp(&a.score).then(b.size.cmp(&a.size)));
    let smallest = out.iter().min_by_key(|f| f.size).cloned();
    out.truncate(KEEP);
    if let Some(s) = smallest {
        if !out.iter().any(|o| o.username == s.username && o.path == s.path) {
            out.push(s);
        }
    }
    out
}

fn wanted_has_no_dash(title: &str) -> bool {
    !parse_title(title).main.contains(" - ")
}

// ---- runner -----------------------------------------------------------------------------------

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FindState {
    Waiting,
    Searching,
    Found,
    NotFound,
    Failed,
    /// The user cancelled this song's search.
    Skipped,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FindResult {
    pub ref_key: String,
    /// The song, as the playlist's source names it (for the Downloads panel).
    pub title: String,
    pub artists: Vec<String>,
    pub state: FindState,
    /// The search that found it (or the last one tried).
    pub query: String,
    pub files: Vec<Found>,
    pub error: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FindProgress {
    pub playlist_id: i64,
    pub result: FindResult,
    pub done: usize,
    pub total: usize,
    pub running: bool,
}

/// Where to look for a missing song.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum FindMode {
    /// Soulseek (better quality), then YouTube for songs Soulseek doesn't have.
    #[default]
    SoulseekFirst,
    /// YouTube only, at YouTube's quality.
    YouTube,
}

/// Called with (ref key, best file) for each song found, to download it right away.
pub type OnFound = Arc<dyn Fn(&str, &Found) + Send + Sync>;

pub struct Finder {
    events: Events,
    /// Cancel flags of running searches, by playlist.
    running: Mutex<HashMap<i64, Arc<AtomicBool>>>,
    /// Latest result per reference (kept while the app runs, so the screen can come back to it).
    results: Mutex<HashMap<String, FindResult>>,
    /// Songs whose search the user cancelled, and the cancel flag of the search running now.
    skipped: Mutex<std::collections::HashSet<String>>,
    current: Mutex<Option<(String, Arc<AtomicBool>)>>,
}

impl Finder {
    pub fn new(events: Events) -> Arc<Self> {
        Arc::new(Self {
            events,
            running: Mutex::new(HashMap::new()),
            results: Mutex::new(HashMap::new()),
            skipped: Mutex::new(Default::default()),
            current: Mutex::new(None),
        })
    }

    pub fn results(&self, keys: &[String]) -> Vec<FindResult> {
        let r = self.results.lock();
        keys.iter().filter_map(|k| r.get(k).cloned()).collect()
    }

    pub fn is_running(&self, playlist_id: i64) -> bool {
        self.running.lock().contains_key(&playlist_id)
    }

    /// Cancel one song's search: skipped if it's still waiting, stopped if it's running.
    pub fn skip(&self, key: &str) {
        self.skipped.lock().insert(key.to_string());
        if let Some((k, flag)) = self.current.lock().as_ref() {
            if k == key {
                flag.store(true, Ordering::Relaxed);
            }
        }
    }

    pub fn stop(&self, playlist_id: i64) {
        if let Some(flag) = self.running.lock().remove(&playlist_id) {
            flag.store(true, Ordering::Relaxed);
            // Also end the search in progress now rather than after its window.
            if let Some((_, current)) = self.current.lock().as_ref() {
                current.store(true, Ordering::Relaxed);
            }
        }
    }

    fn set(&self, playlist_id: i64, result: FindResult, done: usize, total: usize, running: bool) {
        self.results.lock().insert(result.ref_key.clone(), result.clone());
        self.events.emit("slsk:find", FindProgress { playlist_id, result, done, total, running });
    }

    /// Search for each reference, one after another, on a background thread: Soulseek first when
    /// signed in, then YouTube (when it's on) for what Soulseek doesn't have.
    pub fn start(
        self: &Arc<Self>,
        slsk: Arc<Soulseek>,
        youtube: Option<Arc<YouTube>>,
        playlist_id: i64,
        refs: Vec<(String, TrackRef)>,
        mode: FindMode,
        on_found: Option<OnFound>,
    ) -> usize {
        self.stop(playlist_id);
        let cancel = Arc::new(AtomicBool::new(false));
        self.running.lock().insert(playlist_id, cancel.clone());
        let total = refs.len();
        for (k, r) in &refs {
            self.skipped.lock().remove(k);
            let waiting = FindResult {
                ref_key: k.clone(),
                title: r.title.clone(),
                artists: r.artists.clone(),
                state: FindState::Waiting,
                query: String::new(),
                files: Vec::new(),
                error: None,
            };
            self.set(playlist_id, waiting, 0, total, true);
        }
        let me = self.clone();
        std::thread::spawn(move || {
            for (i, (k, r)) in refs.into_iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let qs = queries(&r);
                let mut result = FindResult {
                    ref_key: k.clone(),
                    title: r.title.clone(),
                    artists: r.artists.clone(),
                    state: FindState::Searching,
                    query: qs.first().cloned().unwrap_or_default(),
                    files: Vec::new(),
                    error: None,
                };
                if me.skipped.lock().contains(&k) {
                    result.state = FindState::Skipped;
                    me.set(playlist_id, result, i + 1, total, true);
                    continue;
                }
                let song_cancel = Arc::new(AtomicBool::new(false));
                *me.current.lock() = Some((k.clone(), song_cancel.clone()));
                me.set(playlist_id, result.clone(), i, total, true);
                let use_slsk = mode == FindMode::SoulseekFirst && slsk.status().state == ConnState::SignedIn;
                for q in qs.iter().filter(|_| use_slsk) {
                    result.query = q.clone();
                    if song_cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    match slsk.search_collect(q, SEARCH_WINDOW, Some(song_cancel.clone())) {
                        Ok(answers) => {
                            result.files = rank(&r, &answers);
                            if !result.files.is_empty() {
                                break;
                            }
                        }
                        Err(e) => {
                            result.error = Some(e);
                            break;
                        }
                    }
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    std::thread::sleep(BETWEEN_SEARCHES);
                }
                // Not on Soulseek (or not looking there): YouTube, when it's on.
                let yt = youtube.as_ref().filter(|y| y.ready());
                if let Some(yt) = yt.filter(|_| result.files.is_empty() && !song_cancel.load(Ordering::Relaxed) && !cancel.load(Ordering::Relaxed)) {
                    result.query = format!("YouTube: {}", qs.first().cloned().unwrap_or_else(|| r.title.clone()));
                    me.set(playlist_id, result.clone(), i, total, true);
                    match yt.find(&r, &song_cancel) {
                        Ok(files) => {
                            if !files.is_empty() {
                                result.error = None;
                            }
                            result.files = files;
                        }
                        Err(e) if result.error.is_none() => result.error = Some(format!("YouTube: {e}")),
                        Err(e) => log::warn!("YouTube search failed: {e}"),
                    }
                }
                *me.current.lock() = None;
                result.state = if song_cancel.load(Ordering::Relaxed) {
                    FindState::Skipped
                } else if result.error.is_some() {
                    FindState::Failed
                } else if result.files.is_empty() {
                    FindState::NotFound
                } else {
                    FindState::Found
                };
                if qs.is_empty() && result.files.is_empty() {
                    result.error = Some("The title is too short to search for.".into());
                    result.state = FindState::Failed;
                }
                let stop_now = yt.is_none() && result.error.as_deref().is_some_and(|e| e.contains("Sign in"));
                if let (FindState::Found, Some(cb)) = (result.state, on_found.as_ref()) {
                    cb(&k, &result.files[0]);
                }
                me.set(playlist_id, result, i + 1, total, true);
                if stop_now {
                    break;
                }
                if use_slsk {
                    std::thread::sleep(BETWEEN_SEARCHES);
                }
            }
            me.running.lock().remove(&playlist_id);
            me.events.emit("slsk:find-done", playlist_id);
        });
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap as Map;

    fn r(title: &str, artists: &[&str], ms: Option<u64>) -> TrackRef {
        TrackRef { title: title.into(), artists: artists.iter().map(|a| a.to_string()).collect(), duration_ms: ms, ..Default::default() }
    }

    fn file(path: &str, size: u64, attrs: &[(u32, u32)]) -> super::super::soulseek::model::RawFile {
        super::super::soulseek::model::RawFile { path: path.into(), size, attrs: attrs.iter().copied().collect::<Map<_, _>>() }
    }

    fn user(name: &str, free: bool, speed: u32, files: Vec<super::super::soulseek::model::RawFile>) -> RawResponse {
        RawResponse { username: name.into(), free_slots: free, speed, files }
    }

    #[test]
    fn search_words() {
        assert_eq!(queries(&r("Into You", &["Ariana Grande"], None)), vec!["ariana grande into you".to_string(), "into you".to_string()]);
        assert_eq!(queries(&r("expectations (feat. Chappell Roan)", &["Olivia Rodrigo", "Chappell Roan"], None))[0], "olivia rodrigo expectations");
        assert_eq!(queries(&r("Shakira, Burna Boy - Dai Dai (Official Video)", &[], None))[0], "shakira burna boy dai");
        let q = queries(&r("Swimming Pools (Drank) - Extended Version", &["Kendrick Lamar"], None));
        assert_eq!(q, vec!["kendrick lamar swimming pools drank extended".to_string(), "swimming pools drank extended".to_string()]);
        assert_eq!(queries(&r("DNA.", &["Kendrick Lamar"], None)), vec!["kendrick lamar dna".to_string()], "a one-word title isn't searched alone");
    }

    #[test]
    fn track_numbers() {
        assert_eq!(strip_track_number("01 - Into You"), "Into You");
        assert_eq!(strip_track_number("1. Into You"), "Into You");
        assert_eq!(strip_track_number("A2 Into You"), "Into You");
        assert_eq!(strip_track_number("1999"), "1999");
        assert_eq!(strip_track_number("22 (Taylor's Version)"), "(Taylor's Version)");
    }

    #[test]
    fn picks_the_right_version_and_quality() {
        let want = r("Swimming Pools (Drank)", &["Kendrick Lamar"], Some(220_000));
        let answers = vec![
            user("slow", false, 100_000, vec![
                file(r"music\Kendrick Lamar - good kid, m.A.A.d city\09 - Swimming Pools (Drank) (Extended Version).flac", 40_000_000, &[(1, 313)]),
                file(r"music\Kendrick Lamar - good kid, m.A.A.d city\09 - Swimming Pools (Drank).flac", 30_000_000, &[(1, 220), (4, 44100), (5, 16)]),
            ]),
            user("fast", true, 3_000_000, vec![
                file(r"mp3\Kendrick Lamar\Swimming Pools (Drank).mp3", 8_800_000, &[(0, 320), (1, 221)]),
                file(r"mp3\Kendrick Lamar\Swimming Pools (Drank) 128.mp3", 3_500_000, &[(0, 128), (1, 221)]),
                file(r"mp3\Other Artist\Swimming Pools (Drank).flac", 30_000_000, &[(1, 220)]),
                file(r"mp3\Kendrick Lamar\Swimming Pools (Drank) (Instrumental).flac", 30_000_000, &[(1, 247)]),
            ]),
        ];
        let found = rank(&want, &answers);
        let paths: Vec<&str> = found.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec![
            r"music\Kendrick Lamar - good kid, m.A.A.d city\09 - Swimming Pools (Drank).flac",
            r"mp3\Kendrick Lamar\Swimming Pools (Drank).mp3",
        ], "FLAC first; no extended, instrumental, 128 kbps or wrong-artist files");
        assert!(found[0].why.starts_with("FLAC 16/44.1 · length matches"), "{}", found[0].why);
    }

    #[test]
    fn smallest_file_is_offered_fourth() {
        let want = r("Dark Red", &["Steve Lacy"], Some(173_000));
        let f = |user: &str, mb: u64, depth: u32| file(&format!(r"{user}\Steve Lacy - Dark Red.flac"), mb * 1_000_000, &[(1, 173), (4, 44100), (5, depth)]);
        let answers = vec![
            user("a", true, 3_000_000, vec![f("a", 33, 24)]),
            user("b", true, 2_900_000, vec![f("b", 32, 24)]),
            user("c", true, 2_800_000, vec![f("c", 31, 24)]),
            user("d", false, 10, vec![f("d", 19, 16)]),
        ];
        let found = rank(&want, &answers);
        assert_eq!(found.len(), 4);
        assert_eq!(found[0].username, "a", "best first");
        assert_eq!((found[3].username.as_str(), found[3].size), ("d", 19_000_000), "smallest added as a 4th choice");
        // When the smallest is already among the best three, nothing is added.
        assert_eq!(rank(&want, &answers[..3]).len(), 3);
    }

    #[test]
    fn length_and_format_rules() {
        let want = r("Into You", &["Ariana Grande"], Some(244_000));
        let answers = vec![user("u", true, 1, vec![
            file(r"Ariana Grande\Into You.flac", 30_000_000, &[(1, 200)]),
            file(r"Ariana Grande\Into You.ogg", 9_000_000, &[(1, 244)]),
            file(r"Ariana Grande\Into You.mp3", 9_800_000, &[(1, 244)]),
        ])];
        let found = rank(&want, &answers);
        assert_eq!(found.len(), 1, "wrong length and OGG rejected; MP3 bitrate estimated from size: {found:#?}");
        assert!(found[0].path.ends_with(".mp3"));
    }
}
