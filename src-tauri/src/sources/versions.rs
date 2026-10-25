//! "Other versions" of a missing song: the original, slowed, sped up, hardstyle, remixes, …
//!
//! Only when the user asks, for one song. Searches Soulseek and YouTube with the version words
//! left out (artist + title, then title alone, since an edit's uploader is often not the song's
//! artist), keeps every acceptable-quality file with the same title whatever its version, and
//! groups them by version. Nothing here is chosen on its own: the song's row keeps the version the
//! playlist asked for until the user picks another one.
//!
//! The recommended file is the best original (no version words) by the playlist's artist, else
//! the best original by anyone, marked "different artist" so the user can check it.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::Serialize;

use super::missing::{file_readings, quality, quality_text, Found};
use super::soulseek::model::{format_of, split_remote, FileAttrs, RawResponse};
use super::soulseek::{ConnState, Soulseek};
use super::youtube::{self, Details, YouTube};
use crate::library::interchange::TrackRef;
use crate::library::matcher::normalize::{key, parse_title, readings, tokens, Reading};

const SEARCH_WINDOW: Duration = Duration::from_secs(10);
const BETWEEN_SEARCHES: Duration = Duration::from_millis(1500);
/// Files kept per version, and versions kept.
const PER_GROUP: usize = 3;
const GROUPS: usize = 8;

/// One file of some version of the song.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VersionFile {
    #[serde(flatten)]
    pub file: Found,
    /// Seconds longer (+) or shorter (−) than the playlist's song, when both lengths are known.
    pub length_delta: Option<i64>,
    /// The playlist's main artist appears in the file's path (or the video's artist/channel).
    pub same_artist: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VersionGroup {
    /// The version words, canonical ("" for the original).
    pub key: String,
    /// "Original", "Hardstyle Slowed", "Slowed Reverb", …
    pub label: String,
    /// The version the playlist asked for.
    pub requested: bool,
    pub files: Vec<VersionFile>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct VersionsResult {
    pub ref_key: String,
    pub groups: Vec<VersionGroup>,
    /// (username, path) of the recommended file.
    pub recommended: Option<(String, String)>,
    pub queries: Vec<String>,
    pub error: Option<String>,
}

/// Display order of known version words; unknown words (remix names, "super") come first.
const ORDER: &[&str] = &[
    "hardstyle", "hardtekk", "phonk", "nightcore", "remix", "remixed", "mix", "edit", "bootleg", "rework", "flip", "vip",
    "mashup", "extended", "live", "acoustic", "unplugged", "instrumental", "acapella", "karaoke", "piano", "orchestral",
    "cover", "demo", "remake", "boosted", "8d", "best", "part", "tiktok", "loop", "slowed", "sped", "reverb", "chopped",
    "screwed",
];

fn group_key(versions: &BTreeSet<String>) -> String {
    versions.iter().cloned().collect::<Vec<_>>().join(" | ")
}

/// "hardstyle slowed" → "Hardstyle Slowed"; "reverb slowed" → "Slowed Reverb"; none → "Original".
pub fn label(versions: &BTreeSet<String>) -> String {
    if versions.is_empty() {
        return "Original".into();
    }
    let word = |w: &str| match w {
        "sped" => "Sped up".to_string(),
        "8d" => "8D".to_string(),
        "vip" => "VIP".to_string(),
        w => {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
        }
    };
    versions
        .iter()
        .map(|v| {
            let ws: Vec<&str> = v.split(' ').collect();
            let mut out: Vec<&str> = ws.iter().copied().filter(|w| !ORDER.contains(w)).collect();
            out.extend(ORDER.iter().copied().filter(|o| ws.contains(o)));
            out.into_iter().map(word).collect::<Vec<_>>().join(" ")
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Search words without the version: (artist + title, title alone).
pub fn queries(r: &TrackRef) -> Vec<String> {
    let rs = readings(&r.title, &r.artists);
    let (title, artist) = match rs.iter().find(|x| x.from_title).filter(|_| r.artists.is_empty()) {
        Some(b) => (b.title_tokens.clone(), tokens(parse_title(&r.title).main.split(" - ").next().unwrap_or(""))),
        None => (rs[0].title_tokens.clone(), r.artists.first().map(|x| tokens(x.split([',', '&']).next().unwrap_or(x))).unwrap_or_default()),
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
    let full = join(&[&artist, &title]);
    if full.chars().count() >= 3 {
        out.push(full);
    }
    let short = join(&[&title]);
    let long_enough = title.len() >= 2 || title.iter().map(|w| w.chars().count()).sum::<usize>() >= 6;
    if long_enough && !out.contains(&short) {
        out.push(short);
    }
    out
}

/// What a song is wanted as: its readings (the playlist's, artist-less ones dropped as in the
/// normal search) and its main artist.
fn wanted(r: &TrackRef) -> (Vec<Reading>, Option<String>) {
    let mut w = readings(&r.title, &r.artists);
    if r.artists.is_empty() && parse_title(&r.title).main.contains(" - ") {
        w.retain(|x| x.from_title);
    }
    let artist = w.iter().find_map(|x| x.artists.first().cloned());
    (w, artist)
}

/// The version of a file with the same title as the song, whatever its version.
fn version_of(file: &[Reading], wanted: &[Reading]) -> Option<BTreeSet<String>> {
    file.iter()
        .find(|f| wanted.iter().any(|w| !w.title_key.is_empty() && f.title_key == w.title_key))
        .map(|f| f.versions.clone())
}

fn delta(r: &TrackRef, len: Option<u32>) -> Option<i64> {
    match (r.duration_ms, len) {
        (Some(ms), Some(len)) => Some(len as i64 - (ms as f64 / 1000.0).round() as i64),
        _ => None,
    }
}

/// One Soulseek file: its version, or `None` when it's another song or below FLAC / 320 MP3.
pub fn judge(r: &TrackRef, user: &RawResponse, path: &str, size: u64, attrs: &FileAttrs) -> Option<(BTreeSet<String>, VersionFile)> {
    let (want, artist) = wanted(r);
    let (folder, name) = split_remote(path);
    let format = format_of(name);
    let q = quality(&format, attrs, size)?;
    let versions = version_of(&file_readings(path), &want)?;
    let same_artist = artist.as_ref().is_none_or(|a| key(path).contains(a.as_str()));
    let speed_bonus = (user.speed as f64 / 2_000_000.0).min(1.0) * 5.0;
    let score = q as f64 * 100.0 + if same_artist { 30.0 } else { 0.0 } + if user.free_slots { 10.0 } else { 0.0 } + speed_bonus;
    let why = format!("{} · {}", quality_text(&format, attrs), if user.free_slots { "free slot" } else { "in the user's queue" });
    let file = Found {
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
        why,
    };
    Some((versions, VersionFile { file, length_delta: delta(r, attrs.length), same_artist }))
}

/// One YouTube video: its version, or `None` when it's another song.
pub fn judge_video(r: &TrackRef, d: &Details) -> Option<(BTreeSet<String>, VersionFile)> {
    let (want, artist) = wanted(r);
    let mut file = readings(&d.title, &[]);
    if !d.artist.is_empty() {
        file.extend(readings(&d.title, &[d.artist.clone()]));
    }
    let versions = version_of(&file, &want)?;
    let same_artist = artist.as_ref().is_none_or(|a| key(&format!("{} {} {}", d.artist, d.channel, d.title)).contains(a.as_str()));
    let mut f = youtube::found(d, None);
    f.why = "AAC ~128 kbps".into();
    f.score = 50.0 + if same_artist { 30.0 } else { 0.0 };
    Some((versions, VersionFile { length_delta: delta(r, d.duration), same_artist, file: f }))
}

/// Groups files by version: the original first, then the playlist's version, then the rest by
/// their best file. Best files first in each group, at most `PER_GROUP` of them.
pub fn group(r: &TrackRef, files: Vec<(BTreeSet<String>, VersionFile)>) -> (Vec<VersionGroup>, Option<(String, String)>) {
    let requested = group_key(&readings(&r.title, &r.artists)[0].versions);
    let mut by: HashMap<String, (BTreeSet<String>, Vec<VersionFile>)> = HashMap::new();
    for (v, f) in files {
        let g = by.entry(group_key(&v)).or_insert_with(|| (v, Vec::new()));
        if !g.1.iter().any(|o| o.file.username == f.file.username && o.file.path == f.file.path) {
            g.1.push(f);
        }
    }
    let mut groups: Vec<VersionGroup> = by
        .into_iter()
        .map(|(k, (v, mut files))| {
            files.sort_by(|a, b| b.file.score.total_cmp(&a.file.score).then(b.file.size.cmp(&a.file.size)));
            files.truncate(PER_GROUP);
            VersionGroup { requested: k == requested, label: label(&v), key: k, files }
        })
        .collect();
    let best = |g: &VersionGroup| g.files.first().map(|f| f.file.score).unwrap_or(0.0);
    groups.sort_by(|a, b| {
        (b.key.is_empty(), b.requested).cmp(&(a.key.is_empty(), a.requested)).then(best(b).total_cmp(&best(a))).then(a.label.cmp(&b.label))
    });
    groups.truncate(GROUPS);
    // Sorted by score, so a same-artist original comes before one by someone else.
    let recommended = groups.iter().find(|g| g.key.is_empty()).and_then(|g| g.files.first()).map(|f| (f.file.username.clone(), f.file.path.clone()));
    (groups, recommended)
}

/// Every version of the song in a set of Soulseek answers.
pub fn collect(r: &TrackRef, responses: &[RawResponse]) -> Vec<(BTreeSet<String>, VersionFile)> {
    responses
        .iter()
        .flat_map(|u| u.files.iter().map(move |f| (u, f)))
        .filter_map(|(u, f)| judge(r, u, &f.path, f.size, &FileAttrs::from_codes(&f.attrs)))
        .collect()
}

/// Runs one song's version search at a time; a new one (or closing the popup) cancels the last.
#[derive(Default)]
pub struct Versions {
    current: Mutex<Option<Arc<AtomicBool>>>,
    /// Results kept while the app runs, so reopening the popup is instant.
    results: Mutex<HashMap<String, VersionsResult>>,
}

impl Versions {
    pub fn cached(&self, ref_key: &str) -> Option<VersionsResult> {
        self.results.lock().get(ref_key).cloned()
    }

    pub fn cancel(&self) {
        if let Some(c) = self.current.lock().take() {
            c.store(true, Ordering::Relaxed);
        }
    }

    /// Searches Soulseek (when signed in) and YouTube (when it's on) at the same time. Blocks.
    pub fn search(&self, slsk: Arc<Soulseek>, yt: Option<Arc<YouTube>>, ref_key: String, r: TrackRef) -> VersionsResult {
        self.cancel();
        let cancel = Arc::new(AtomicBool::new(false));
        *self.current.lock() = Some(cancel.clone());
        let qs = queries(&r);
        let mut result = VersionsResult { ref_key: ref_key.clone(), queries: qs.clone(), ..Default::default() };
        if qs.is_empty() {
            result.error = Some("The title is too short to search for.".into());
            return result;
        }
        let yt = yt.filter(|y| y.ready());
        let from_yt = yt.map(|yt| {
            let (r, cancel, skip) = (r.clone(), cancel.clone(), r.service_ids.get("youtube").cloned());
            std::thread::spawn(move || yt.find_versions(&r, skip.as_deref(), &cancel))
        });
        let mut files = Vec::new();
        let mut errors = Vec::new();
        if slsk.status().state == ConnState::SignedIn {
            for (i, q) in qs.iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                if i > 0 {
                    std::thread::sleep(BETWEEN_SEARCHES);
                }
                match slsk.search_collect(q, SEARCH_WINDOW, Some(cancel.clone())) {
                    Ok(answers) => files.extend(collect(&r, &answers)),
                    Err(e) => {
                        errors.push(e);
                        break;
                    }
                }
            }
        }
        if let Some(h) = from_yt {
            match h.join() {
                Ok(Ok(v)) => files.extend(v.iter().filter_map(|d| judge_video(&r, d))),
                Ok(Err(e)) => errors.push(format!("YouTube: {e}")),
                Err(_) => errors.push("YouTube search failed.".into()),
            }
        }
        let (groups, recommended) = group(&r, files);
        result.groups = groups;
        result.recommended = recommended;
        if result.groups.is_empty() {
            result.error = errors.into_iter().next();
        }
        if !cancel.load(Ordering::Relaxed) {
            self.results.lock().insert(ref_key, result.clone());
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::soulseek::model::RawFile;

    fn r(title: &str, artists: &[&str], ms: Option<u64>) -> TrackRef {
        TrackRef { title: title.into(), artists: artists.iter().map(|a| a.to_string()).collect(), duration_ms: ms, ..Default::default() }
    }

    fn file(path: &str, mb: u64, attrs: &[(u32, u32)]) -> RawFile {
        RawFile { path: path.into(), size: mb * 1_000_000, attrs: attrs.iter().copied().collect() }
    }

    fn user(name: &str, files: Vec<RawFile>) -> RawResponse {
        RawResponse { username: name.into(), free_slots: true, speed: 1_000_000, files }
    }

    fn set(v: &[&str]) -> BTreeSet<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn labels() {
        assert_eq!(label(&set(&[])), "Original");
        assert_eq!(label(&set(&["hardstyle slowed"])), "Hardstyle Slowed");
        assert_eq!(label(&set(&["reverb slowed"])), "Slowed Reverb");
        assert_eq!(label(&set(&["sped"])), "Sped up");
        assert_eq!(label(&set(&["reverb slowed super"])), "Super Slowed Reverb");
        assert_eq!(label(&set(&["cinematic hoodtrap mylancore remix"])), "Cinematic Hoodtrap Mylancore Remix");
    }

    #[test]
    fn search_words_leave_the_version_out() {
        assert_eq!(queries(&r("BANG BANG BANG (Hardstyle Slowed)", &["Enmity"], None)), vec!["enmity bang", "bang"]);
        assert_eq!(queries(&r("Dark Red (Sped Up)", &["Steve Lacy"], None)), vec!["steve lacy dark red", "dark red"]);
    }

    #[test]
    fn groups_every_version_with_the_original_first() {
        let want = r("BANG BANG BANG (Hardstyle Slowed)", &["Enmity"], Some(130_000));
        let answers = vec![
            user("a", vec![
                file(r"kpop\BIGBANG - MADE\02 - BANG BANG BANG.flac", 30, &[(1, 220), (4, 44100), (5, 16)]),
                file(r"kpop\BIGBANG - MADE\02 - BANG BANG BANG.mp3", 9, &[(0, 320), (1, 220)]),
                file(r"kpop\BIGBANG\BANG BANG BANG 128.mp3", 3, &[(0, 128), (1, 220)]),
                file(r"kpop\BIGBANG\LOSER.flac", 30, &[(1, 220)]),
            ]),
            user("b", vec![
                file(r"edits\Enmity - BANG BANG BANG (Hardstyle Slowed).mp3", 6, &[(0, 320), (1, 131)]),
                file(r"edits\Enmity - BANG BANG BANG (Hardstyle).flac", 20, &[(1, 110)]),
                file(r"edits\Enmity - BANG BANG BANG.flac", 25, &[(1, 180)]),
                file(r"edits\Someone - BANG BANG BANG (Sped Up).mp3", 6, &[(0, 320), (1, 180)]),
            ]),
        ];
        let (groups, rec) = group(&want, collect(&want, &answers));
        let labels: Vec<&str> = groups.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(labels, vec!["Original", "Hardstyle Slowed", "Hardstyle", "Sped up"]);
        assert!(groups[1].requested && !groups[0].requested);
        let original = &groups[0].files;
        assert_eq!(original.len(), 3, "128 kbps MP3 and other songs left out: {original:#?}");
        assert!(original[0].same_artist && original[0].file.path.starts_with("edits"), "same artist first");
        assert_eq!(original[1].file.format, "FLAC");
        assert!(!original[1].same_artist);
        assert_eq!(original[1].length_delta, Some(90));
        assert_eq!(rec, Some(("b".into(), r"edits\Enmity - BANG BANG BANG.flac".into())));
        assert_eq!(groups[1].files[0].length_delta, Some(1));
    }

    #[test]
    fn recommends_another_artists_original_when_the_artist_has_none() {
        let want = r("BANG BANG BANG (Hardstyle Slowed)", &["Enmity"], None);
        let answers = vec![user("a", vec![file(r"kpop\BIGBANG - MADE\02 - BANG BANG BANG.flac", 30, &[(1, 220)])])];
        let (groups, rec) = group(&want, collect(&want, &answers));
        assert_eq!(groups.len(), 1);
        assert!(!groups[0].files[0].same_artist);
        assert_eq!(rec.map(|(u, _)| u), Some("a".into()));
    }

    #[test]
    fn videos() {
        let want = r("BANG BANG BANG (Hardstyle Slowed)", &["Enmity"], Some(130_000));
        let d = |title: &str, artist: &str, secs: u32| Details {
            id: "abcdefghij1".into(),
            title: title.into(),
            artist: artist.into(),
            album: String::new(),
            duration: Some(secs),
            channel: artist.into(),
        };
        let (v, f) = judge_video(&want, &d("BANG BANG BANG", "BIGBANG", 220)).unwrap();
        assert!(v.is_empty() && !f.same_artist);
        assert_eq!((f.file.why.as_str(), f.length_delta), ("AAC ~128 kbps", Some(90)));
        assert!(judge_video(&want, &d("FANTASTIC BABY", "BIGBANG", 230)).is_none());
    }
}
