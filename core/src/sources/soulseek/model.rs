//! Pure helpers for Soulseek: file attributes, grouping search results, safe local names and
//! the download queue's states. No network here, so all of it is unit-tested.

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;

use crate::library::model::is_audio_file;

/// What a search result says about one file. Soulseek sends these as numbered attributes
/// (codes from the Nicotine+ protocol doc): 0 bitrate (kbps), 1 length (s), 2 VBR flag,
/// 4 sample rate (Hz), 5 bit depth. Code 3 (encoder) is unused.
#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileAttrs {
    pub bitrate: Option<u32>,
    pub length: Option<u32>,
    pub vbr: bool,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u32>,
}

impl FileAttrs {
    pub fn from_codes(attrs: &HashMap<u32, u32>) -> Self {
        // Zero means "unknown" in practice; some clients send 0 for every field.
        let get = |code| attrs.get(&code).copied().filter(|v| *v > 0);
        Self {
            bitrate: get(0),
            length: get(1),
            vbr: get(2) == Some(1),
            sample_rate: get(4),
            bit_depth: get(5),
        }
    }
}

/// Upper-case extension used as the format label ("FLAC", "MP3").
pub fn format_of(name: &str) -> String {
    Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_uppercase()
}

/// Splits a remote path into (folder, file name). Soulseek paths use `\`, but some clients
/// send `/`; the folder is everything before the last separator.
pub fn split_remote(path: &str) -> (&str, &str) {
    match path.rfind(['\\', '/']) {
        Some(i) => (&path[..i], &path[i + 1..]),
        None => ("", path),
    }
}

const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1",
    "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fn bad_char(c: char) -> bool {
    c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
}

/// True if `name` can be used as-is for one path component on Windows: not empty, not `.`/`..`,
/// no characters Windows rejects (which also rules out drive prefixes like `C:`), no reserved
/// device name, no trailing dot or space.
pub fn is_safe_component(name: &str) -> bool {
    if name.is_empty() || name == "." || name == ".." || name.len() > 255 {
        return false;
    }
    if name.chars().any(bad_char) || name.ends_with(['.', ' ']) {
        return false;
    }
    let stem = name.split('.').next().unwrap_or("").trim_end();
    !RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem))
}

/// Turns a remote folder name into one we can create locally, replacing what Windows rejects.
/// Returns `None` if nothing usable is left.
pub fn sanitize_component(name: &str) -> Option<String> {
    let mut s: String = name.chars().map(|c| if bad_char(c) { '_' } else { c }).collect();
    s = s.trim().trim_end_matches(['.', ' ']).to_string();
    if s.chars().count() > 120 {
        s = s.chars().take(120).collect::<String>().trim_end_matches(['.', ' ']).to_string();
    }
    if s.is_empty() || s.chars().all(|c| c == '_' || c == '.') {
        return None;
    }
    if !is_safe_component(&s) {
        s = format!("_{s}");
    }
    is_safe_component(&s).then_some(s)
}

/// Local folder (under the download folder) for a remote file: the name of the folder it sits
/// in, so an album keeps its own folder. Files at the top of a share go straight in.
pub fn local_subfolder(remote_path: &str) -> Option<String> {
    let (folder, _) = split_remote(remote_path);
    let (_, last) = split_remote(folder);
    sanitize_component(last)
}

/// One file in a search result, ready for the UI.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResultFile {
    /// Full remote path; what we ask the user for.
    pub path: String,
    pub name: String,
    pub size: u64,
    pub format: String,
    #[serde(flatten)]
    pub attrs: FileAttrs,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResultFolder {
    pub path: String,
    pub files: Vec<ResultFile>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResultUser {
    pub username: String,
    /// Free upload slots: 0 means downloads wait in the user's queue.
    pub free_slots: bool,
    /// Average upload speed in bytes per second, as reported by the server.
    pub speed: u32,
    pub folders: Vec<ResultFolder>,
}

/// One peer's answer, independent of the protocol crate's types so this stays testable.
pub struct RawResponse {
    pub username: String,
    pub free_slots: bool,
    pub speed: u32,
    pub files: Vec<RawFile>,
}

pub struct RawFile {
    pub path: String,
    pub size: u64,
    pub attrs: HashMap<u32, u32>,
}

/// Groups search responses by user, then folder. Keeps only audio files we can play and whose
/// names are safe to save; drops users left with nothing. Users with a free slot come first,
/// then faster ones. A user who answered twice is merged.
pub fn group_results(responses: Vec<RawResponse>) -> Vec<ResultUser> {
    let mut users: Vec<ResultUser> = Vec::new();
    for r in responses {
        let idx = match users.iter().position(|u| u.username == r.username) {
            Some(i) => i,
            None => {
                users.push(ResultUser { username: r.username, free_slots: r.free_slots, speed: r.speed, folders: Vec::new() });
                users.len() - 1
            }
        };
        let user = &mut users[idx];
        user.free_slots |= r.free_slots;
        user.speed = user.speed.max(r.speed);
        for f in r.files {
            let (folder, name) = split_remote(&f.path);
            if !is_audio_file(Path::new(name)) || !is_safe_component(name) {
                continue;
            }
            if user.folders.iter().any(|d| d.files.iter().any(|x| x.path == f.path)) {
                continue;
            }
            let file = ResultFile {
                name: name.to_string(),
                format: format_of(name),
                attrs: FileAttrs::from_codes(&f.attrs),
                size: f.size,
                path: f.path.clone(),
            };
            match user.folders.iter_mut().find(|d| d.path == folder) {
                Some(d) => d.files.push(file),
                None => user.folders.push(ResultFolder { path: folder.to_string(), files: vec![file] }),
            }
        }
    }
    users.retain(|u| !u.folders.is_empty());
    for u in &mut users {
        u.folders.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));
        for d in &mut u.folders {
            d.files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        }
    }
    users.sort_by(|a, b| b.free_slots.cmp(&a.free_slots).then(b.speed.cmp(&a.speed)));
    users
}

/// Where a download is in its life.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DownloadState {
    /// Asked for; waiting for the user to start sending (their queue, or connecting).
    Queued,
    Downloading,
    Done,
    Failed,
    Cancelled,
}

impl DownloadState {
    pub fn is_finished(self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Cancelled)
    }

    pub fn can_cancel(self) -> bool {
        !self.is_finished()
    }

    pub fn can_retry(self) -> bool {
        matches!(self, Self::Failed | Self::Cancelled)
    }

    /// The state after a progress report from the transfer. Finished states stay put, so a late
    /// report from a cancelled transfer can't bring it back to life.
    pub fn next(self, reported: DownloadState) -> DownloadState {
        if self.is_finished() {
            self
        } else {
            reported
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attrs(pairs: &[(u32, u32)]) -> HashMap<u32, u32> {
        pairs.iter().copied().collect()
    }

    #[test]
    fn attributes_lossy_and_lossless() {
        let mp3 = FileAttrs::from_codes(&attrs(&[(0, 320), (1, 245), (2, 0)]));
        assert_eq!(mp3, FileAttrs { bitrate: Some(320), length: Some(245), vbr: false, sample_rate: None, bit_depth: None });
        let flac = FileAttrs::from_codes(&attrs(&[(1, 300), (4, 44100), (5, 24)]));
        assert_eq!(flac.sample_rate, Some(44100));
        assert_eq!(flac.bit_depth, Some(24));
        assert_eq!(flac.bitrate, None);
        let vbr = FileAttrs::from_codes(&attrs(&[(0, 245), (2, 1)]));
        assert!(vbr.vbr);
    }

    #[test]
    fn zero_and_unknown_attributes_are_ignored() {
        let a = FileAttrs::from_codes(&attrs(&[(0, 0), (1, 0), (3, 7), (99, 5)]));
        assert_eq!(a, FileAttrs::default());
    }

    #[test]
    fn split_remote_paths() {
        assert_eq!(split_remote(r"@@abc\Music\Artist\Album\01 Song.flac"), (r"@@abc\Music\Artist\Album", "01 Song.flac"));
        assert_eq!(split_remote("music/Album/02.mp3"), ("music/Album", "02.mp3"));
        assert_eq!(split_remote("loose.mp3"), ("", "loose.mp3"));
        assert_eq!(split_remote(r"Album\"), ("Album", ""));
    }

    #[test]
    fn format_label() {
        assert_eq!(format_of("a.flac"), "FLAC");
        assert_eq!(format_of("b.Mp3"), "MP3");
        assert_eq!(format_of("noext"), "");
    }

    #[test]
    fn safe_components() {
        assert!(is_safe_component("01 - Song (Live).flac"));
        assert!(is_safe_component("Ünïcödé ✓.mp3"));
        for bad in ["", ".", "..", "C:evil.mp3", "a?b.mp3", "con.mp3", "CON", "Lpt1.flac", "x.mp3.", "x.mp3 ", "a\u{0}b"] {
            assert!(!is_safe_component(bad), "{bad:?} should be unsafe");
        }
        assert!(is_safe_component("console.mp3"));
        assert!(!is_safe_component(&"a".repeat(256)));
    }

    #[test]
    fn sanitize_folder_names() {
        assert_eq!(sanitize_component("Artist - Album [FLAC]").as_deref(), Some("Artist - Album [FLAC]"));
        assert_eq!(sanitize_component("What? Why: Now").as_deref(), Some("What_ Why_ Now"));
        assert_eq!(sanitize_component("Album...  ").as_deref(), Some("Album"));
        assert_eq!(sanitize_component("CON").as_deref(), Some("_CON"));
        assert_eq!(sanitize_component("..").as_deref(), None);
        assert_eq!(sanitize_component("").as_deref(), None);
        assert_eq!(sanitize_component("???").as_deref(), None);
        assert!(sanitize_component(&"x".repeat(400)).unwrap().chars().count() <= 120);
    }

    #[test]
    fn subfolder_is_the_album_folder() {
        assert_eq!(local_subfolder(r"@@abc\Music\Artist\Album (2001)\01.flac").as_deref(), Some("Album (2001)"));
        assert_eq!(local_subfolder("top.mp3"), None);
        assert_eq!(local_subfolder(r"@@abc\..\x.mp3"), None);
    }

    fn file(path: &str, size: u64) -> RawFile {
        RawFile { path: path.into(), size, attrs: attrs(&[(0, 320)]) }
    }

    #[test]
    fn grouping_by_user_and_folder() {
        let responses = vec![
            RawResponse {
                username: "slow".into(),
                free_slots: true,
                speed: 100,
                files: vec![file(r"s\A\2.mp3", 2), file(r"s\A\1.mp3", 1), file(r"s\B\1.flac", 3), file(r"s\A\cover.jpg", 9)],
            },
            RawResponse { username: "busy".into(), free_slots: false, speed: 9_000, files: vec![file(r"b\X\1.mp3", 1)] },
            RawResponse { username: "fast".into(), free_slots: true, speed: 5_000, files: vec![file(r"f\Y\1.ogg", 1)] },
            RawResponse { username: "junk".into(), free_slots: true, speed: 1, files: vec![file(r"j\readme.txt", 1)] },
        ];
        let users = group_results(responses);
        let names: Vec<_> = users.iter().map(|u| u.username.as_str()).collect();
        assert_eq!(names, ["fast", "slow", "busy"]);
        let slow = &users[1];
        assert_eq!(slow.folders.len(), 2);
        assert_eq!(slow.folders[0].path, r"s\A");
        let a: Vec<_> = slow.folders[0].files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(a, ["1.mp3", "2.mp3"]);
        assert_eq!(slow.folders[1].files[0].format, "FLAC");
        assert_eq!(slow.folders[1].files[0].attrs.bitrate, Some(320));
    }

    #[test]
    fn grouping_merges_repeat_answers_and_drops_unsafe_names() {
        let responses = vec![
            RawResponse { username: "u".into(), free_slots: false, speed: 1, files: vec![file(r"u\A\1.mp3", 1)] },
            RawResponse {
                username: "u".into(),
                free_slots: true,
                speed: 5,
                files: vec![file(r"u\A\1.mp3", 1), file(r"u\A\2.mp3", 1), file(r"u\A\..", 1), file(r"u\A\con.mp3", 1)],
            },
        ];
        let users = group_results(responses);
        assert_eq!(users.len(), 1);
        assert!(users[0].free_slots);
        assert_eq!(users[0].speed, 5);
        assert_eq!(users[0].folders[0].files.len(), 2);
    }

    #[test]
    fn queue_state_transitions() {
        use DownloadState::*;
        assert_eq!(Queued.next(Downloading), Downloading);
        assert_eq!(Downloading.next(Done), Done);
        assert_eq!(Downloading.next(Queued), Queued);
        // Finished states don't move on late reports.
        assert_eq!(Cancelled.next(Downloading), Cancelled);
        assert_eq!(Done.next(Failed), Done);
        assert!(Failed.can_retry() && Cancelled.can_retry());
        assert!(!Done.can_retry() && !Downloading.can_retry());
        assert!(Queued.can_cancel() && Downloading.can_cancel());
        assert!(!Done.can_cancel());
    }
}
