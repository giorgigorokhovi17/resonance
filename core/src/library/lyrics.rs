//! Lyrics from the music files themselves: a `.lrc` file next to the song (same name), or the
//! lyrics stored in its tags. Lyrics the user looked up online (sources/lyrics_search.rs) are kept
//! in the database and used when the files have none. Timed LRC lines let the UI follow the song;
//! plain text is shown as it is.

use std::path::Path;

use lofty::config::ParseOptions;
use lofty::prelude::*;
use lofty::probe::Probe;
use serde::Serialize;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LyricLine {
    /// Start of the line in milliseconds, for synced lyrics.
    pub time_ms: Option<u64>,
    pub text: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Lyrics {
    pub lines: Vec<LyricLine>,
    /// Every line has a time.
    pub synced: bool,
    /// "lrc" (a file next to the song), "tags", or where they were found online ("lrclib").
    pub source: String,
}

/// Reads `[mm:ss.xx]` (or `[mm:ss]`, `[mm:ss:xx]`) into milliseconds.
fn parse_time(t: &str) -> Option<u64> {
    let (m, rest) = t.split_once(':')?;
    let m: u64 = m.trim().parse().ok()?;
    let (s, frac) = match rest.find(['.', ':']) {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => (rest, ""),
    };
    let s: u64 = s.trim().parse().ok()?;
    if s >= 60 {
        return None;
    }
    let frac_ms = if frac.is_empty() {
        0
    } else {
        let digits: String = frac.chars().take_while(|c| c.is_ascii_digit()).take(3).collect();
        if digits.is_empty() {
            return None;
        }
        digits.parse::<u64>().ok()? * 10u64.pow(3 - digits.len() as u32)
    };
    Some(m * 60_000 + s * 1000 + frac_ms)
}

/// Removes word timings (`<mm:ss.xx>`) of enhanced LRC.
fn strip_word_times(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('<') {
        match rest[i..].find('>') {
            Some(j) if parse_time(&rest[i + 1..i + j]).is_some() => {
                out.push_str(&rest[..i]);
                rest = &rest[i + j + 1..];
            }
            _ => {
                out.push_str(&rest[..=i]);
                rest = &rest[i + 1..];
            }
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// LRC header tags; anything else in brackets ("[Chorus: x2]") is part of the lyrics.
const META_KEYS: &[&str] = &["ar", "al", "ti", "au", "by", "re", "ve", "length", "offset", "tool", "la", "id", "#"];

/// Parses LRC text. Lines without timestamps make the result plain text; metadata lines
/// (`[ar:…]`, `[ti:…]`, …) are dropped and `[offset:…]` is applied.
pub fn parse(text: &str) -> Vec<LyricLine> {
    let text = text.trim_start_matches('\u{feff}');
    let mut offset: i64 = 0;
    let mut timed: Vec<LyricLine> = Vec::new();
    let mut plain: Vec<LyricLine> = Vec::new();
    for raw in text.lines() {
        let mut line = raw.trim();
        let mut times = Vec::new();
        let mut meta = false;
        while line.starts_with('[') {
            let Some(end) = line.find(']') else { break };
            let tag = &line[1..end];
            if let Some(t) = parse_time(tag) {
                times.push(t);
            } else if let Some((k, v)) =
                tag.split_once(':').filter(|(k, _)| META_KEYS.contains(&k.trim().to_ascii_lowercase().as_str()))
            {
                if k.trim().eq_ignore_ascii_case("offset") {
                    offset = v.trim().parse().unwrap_or(0);
                }
                meta = true;
            } else {
                break;
            }
            line = line[end + 1..].trim_start();
        }
        if meta && times.is_empty() {
            continue;
        }
        let words = strip_word_times(line);
        if times.is_empty() {
            plain.push(LyricLine { time_ms: None, text: words });
        } else {
            for t in times {
                // A positive offset shows lyrics earlier.
                let at = (t as i64 - offset).max(0) as u64;
                timed.push(LyricLine { time_ms: Some(at), text: words.clone() });
            }
        }
    }
    let lines = if timed.is_empty() {
        plain
    } else {
        timed.sort_by_key(|l| l.time_ms);
        timed
    };
    // No blank lines at either end.
    let first = lines.iter().position(|l| !l.text.is_empty()).unwrap_or(lines.len());
    let last = lines.iter().rposition(|l| !l.text.is_empty()).map(|i| i + 1).unwrap_or(first);
    lines[first..last].to_vec()
}

pub fn from_text(text: &str, source: &str) -> Option<Lyrics> {
    let lines = parse(text);
    if lines.iter().all(|l| l.text.is_empty()) {
        return None;
    }
    let synced = lines.iter().all(|l| l.time_ms.is_some());
    Some(Lyrics { lines, synced, source: source.into() })
}

/// A `.lrc` file with the same name as the song, in the same folder (any case of the extension).
fn sidecar(path: &Path) -> Option<String> {
    let dir = path.parent()?;
    let stem = path.file_stem()?.to_string_lossy().to_lowercase();
    let entry = std::fs::read_dir(dir).ok()?.flatten().find(|e| {
        let p = e.path();
        p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lrc"))
            && p.file_stem().is_some_and(|s| s.to_string_lossy().to_lowercase() == stem)
    })?;
    if entry.metadata().ok()?.len() > 1024 * 1024 {
        return None;
    }
    Some(String::from_utf8_lossy(&std::fs::read(entry.path()).ok()?).into_owned())
}

fn from_tags(path: &Path) -> Option<String> {
    let tagged = Probe::open(path).ok()?.options(ParseOptions::new().read_cover_art(false)).guess_file_type().ok()?.read().ok()?;
    tagged
        .tags()
        .iter()
        .find_map(|t| t.get_string(ItemKey::Lyrics).or_else(|| t.get_string(ItemKey::UnsyncLyrics)))
        .map(str::to_string)
}

/// Lyrics for a song: the `.lrc` file next to it first, then its tags, then lyrics found online
/// earlier (`saved`: their text and source).
pub fn read(path: &Path, saved: Option<(String, String)>) -> Option<Lyrics> {
    sidecar(path)
        .and_then(|t| from_text(&t, "lrc"))
        .or_else(|| from_tags(path).and_then(|t| from_text(&t, "tags")))
        .or_else(|| saved.and_then(|(text, source)| from_text(&text, &source)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms: u64, t: &str) -> LyricLine {
        LyricLine { time_ms: Some(ms), text: t.into() }
    }

    #[test]
    fn synced_lrc() {
        let lrc = "\u{feff}[ti:Song]\n[ar:Someone]\n[00:12.50]First line\n[00:15.1]Second\n\n[01:02.003][00:20.00]Chorus\n[00:21]<00:21.00>Word <00:21.50>by word\n";
        assert_eq!(
            parse(lrc),
            vec![at(12_500, "First line"), at(15_100, "Second"), at(20_000, "Chorus"), at(21_000, "Word by word"), at(62_003, "Chorus")]
        );
        let l = from_text(lrc, "lrc").unwrap();
        assert!(l.synced);
    }

    #[test]
    fn offset_moves_lines_earlier() {
        assert_eq!(parse("[offset:+500]\n[00:10.00]A\n[00:00.20]B"), vec![at(0, "B"), at(9_500, "A")]);
        assert_eq!(parse("[offset:-1000]\n[00:10.00]A"), vec![at(11_000, "A")]);
    }

    #[test]
    fn plain_text_stays_as_written() {
        let l = from_text("\n\nVerse one\nstill verse\n\nChorus [x2]\n\n", "tags").unwrap();
        assert!(!l.synced);
        let text: Vec<&str> = l.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(text, vec!["Verse one", "still verse", "", "Chorus [x2]"]);
        assert!(from_text("  \n [ar:Only metadata]\n", "lrc").is_none());
    }

    #[test]
    fn bad_timestamps_are_text() {
        assert_eq!(parse_time("01:75.00"), None);
        assert_eq!(parse_time("1:05"), Some(65_000));
        assert_eq!(parse_time("00:01:50"), Some(1_500));
        assert_eq!(parse("[Chorus: x2]\nla")[0].text, "[Chorus: x2]");
        assert_eq!(parse("[Intro]\nhello"), vec![LyricLine { time_ms: None, text: "[Intro]".into() }, LyricLine { time_ms: None, text: "hello".into() }]);
    }

    #[test]
    fn sidecar_lrc_is_found_by_name() {
        let dir = std::env::temp_dir().join(format!("resonance-lyrics-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let song = dir.join("01 Song.flac");
        std::fs::write(&song, b"not really audio").unwrap();
        assert_eq!(read(&song, None), None);
        let saved = read(&song, Some(("[00:03.00]Found".into(), "lrclib".into()))).unwrap();
        assert_eq!((saved.source.as_str(), saved.lines), ("lrclib", vec![at(3_000, "Found")]));
        std::fs::write(dir.join("01 song.LRC"), "[00:01.00]Hello").unwrap();
        let l = read(&song, Some(("[00:03.00]Found".into(), "lrclib".into()))).unwrap();
        assert_eq!((l.source.as_str(), l.lines), ("lrc", vec![at(1_000, "Hello")]));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
