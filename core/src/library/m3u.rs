//! M3U / M3U8 playlists.
//!
//! Reads plain and extended M3U: `#EXTINF:<seconds>[ attrs],<display>`, `#EXTART:`, `#EXTALB:` and
//! `#PLAYLIST:`. Entries are kept as written (see [`canonical_location`]); plain paths are not
//! percent-decoded here, because a real file can be named `100%AB.mp3`. Resolution decides.
//!
//! Writes extended M3U8 with `#EXTART`/`#EXTALB`, so artist and album survive a round trip.
//! Players that don't know those directives (VLC, foobar2000) ignore them. `#EXTART` is one
//! string: a list of artists is written joined with "; " and read back as that one string, the
//! same as an artist tag like `vivi baby; ovg!`. Splitting it is the matcher's job.

use std::path::Path;

use anyhow::{Context, Result};

use super::interchange::{canonical_location, relative_to, Playlist, PlaylistItem, TrackRef};

const ARTIST_SEP: &str = "; ";

#[derive(Default)]
struct Pending {
    duration_ms: Option<u64>,
    display: Option<String>,
    artist: Option<String>,
    album: Option<String>,
}

/// `<seconds>[ key="value" …],<display>`; the comma inside quoted attribute values doesn't count.
fn parse_extinf(val: &str) -> (Option<u64>, String) {
    let mut quoted = false;
    let split = val.char_indices().find(|&(_, c)| {
        if c == '"' {
            quoted = !quoted;
        }
        c == ',' && !quoted
    });
    let (head, display) = match split {
        Some((i, _)) => (&val[..i], val[i + 1..].trim()),
        None => (val, ""),
    };
    let secs = head.split_whitespace().next().and_then(|s| s.parse::<f64>().ok());
    let duration_ms = secs.filter(|s| s.is_finite() && *s > 0.0).map(|s| (s * 1000.0).round() as u64);
    (duration_ms, display.to_string())
}

pub fn parse_str(text: &str, default_name: &str) -> Playlist {
    let mut pl = Playlist { name: default_name.to_string(), source: "m3u".into(), ..Default::default() };
    let mut pending = Pending::default();
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('#') {
            let (tag, val) = rest.split_once(':').unwrap_or((rest, ""));
            match tag.trim().to_ascii_uppercase().as_str() {
                "EXTINF" => {
                    let (d, display) = parse_extinf(val);
                    pending.duration_ms = d;
                    pending.display = Some(display);
                }
                "EXTART" => pending.artist = Some(val.trim().to_string()),
                "EXTALB" => pending.album = Some(val.trim().to_string()),
                "PLAYLIST" if !val.trim().is_empty() => pl.name = val.trim().to_string(),
                _ => {}
            }
            continue;
        }
        let Some(location) = canonical_location(line) else { continue };
        let p = std::mem::take(&mut pending);
        let artists: Vec<String> = p.artist.filter(|a| !a.is_empty()).into_iter().collect();
        let display = p.display.unwrap_or_default();
        // With an explicit artist, the display string is "<artist> - <title>"; without one it is
        // kept whole (it may or may not be "Artist - Title"; the matcher decides).
        let title = match artists.first().and_then(|a| display.strip_prefix(&format!("{a} - "))) {
            Some(t) => t.to_string(),
            None => display,
        };
        pl.items.push(PlaylistItem {
            track: TrackRef {
                title,
                artists,
                album: p.album.unwrap_or_default(),
                duration_ms: p.duration_ms,
                location: Some(location),
                ..Default::default()
            },
            added_at: None,
        });
    }
    pl
}

fn one_line(s: &str) -> String {
    s.replace(['\r', '\n'], " ")
}

/// Extended M3U8 text. Locations inside `base` (the playlist's folder) are written relative to it.
/// Items without a location can't be represented and are left out.
pub fn write_str(pl: &Playlist, base: Option<&Path>) -> String {
    let mut s = String::from("#EXTM3U\r\n");
    if !pl.name.is_empty() {
        s.push_str(&format!("#PLAYLIST:{}\r\n", one_line(&pl.name)));
    }
    for item in &pl.items {
        let t = &item.track;
        let Some(loc) = &t.location else { continue };
        let secs = t.duration_ms.map(|ms| (ms as f64 / 1000.0).round() as i64).filter(|s| *s > 0).unwrap_or(-1);
        let artists = t.artists.join(ARTIST_SEP);
        let display = if artists.is_empty() { t.title.clone() } else { format!("{artists} - {}", t.title) };
        s.push_str(&format!("#EXTINF:{secs},{}\r\n", one_line(&display)));
        if !artists.is_empty() {
            s.push_str(&format!("#EXTART:{}\r\n", one_line(&artists)));
        }
        if !t.album.is_empty() {
            s.push_str(&format!("#EXTALB:{}\r\n", one_line(&t.album)));
        }
        let loc = relative_to(loc, base);
        if loc.starts_with('#') {
            s.push_str(".\\"); // would otherwise read as a comment
        }
        s.push_str(&one_line(&loc));
        s.push_str("\r\n");
    }
    s
}

/// Write `pl` to `file`. `.m3u` gets a UTF-8 BOM so players that assume the ANSI code page for
/// that extension still read non-ASCII names correctly; `.m3u8` is UTF-8 by definition.
pub fn write_file(file: &Path, pl: &Playlist) -> Result<()> {
    let text = write_str(pl, file.parent());
    let bom = file.extension().is_some_and(|e| e.eq_ignore_ascii_case("m3u"));
    let mut bytes = if bom { vec![0xEF, 0xBB, 0xBF] } else { Vec::new() };
    bytes.extend_from_slice(text.as_bytes());
    std::fs::write(file, bytes).with_context(|| format!("cannot write {}", file.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::interchange::{parse_file, resolve_location, testgen};
    use crate::library::model::Track;
    use proptest::prelude::*;

    fn resolved(file: &Path) -> Vec<String> {
        let base = file.parent().unwrap();
        parse_file(file).unwrap().items.iter().filter_map(|i| resolve_location(i.track.location.as_deref()?, base)).collect()
    }

    #[test]
    fn roundtrip_relative_and_absolute() {
        let dir = std::env::temp_dir().join(format!("m3u-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let inside = dir.join("sub").join("a.flac").to_string_lossy().to_string();
        let outside = "D:\\Music\\b.flac".to_string();
        let tracks = [
            Track { path: inside.clone(), title: "A".into(), artist: "X".into(), duration: 61.4, ..Default::default() },
            Track { path: outside.clone(), title: "B".into(), ..Default::default() },
        ];
        let pl = Playlist {
            name: "list".into(),
            items: tracks.iter().map(|t| PlaylistItem { track: super::super::interchange::track_ref_from_track(t), added_at: None }).collect(),
            ..Default::default()
        };
        let f = dir.join("list.m3u8");
        write_file(&f, &pl).unwrap();
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(text.contains("#EXTINF:61,X - A"));
        assert!(text.contains("#EXTART:X"));
        assert!(text.contains("sub\\a.flac"));
        assert!(!text.starts_with('\u{feff}'), "no BOM for .m3u8");
        assert_eq!(resolved(&f), vec![inside, outside]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_forward_slashes_and_dotdot() {
        let dir = std::env::temp_dir().join(format!("m3u-test2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("x.m3u");
        std::fs::write(&f, "#EXTM3U\n../Music/Album/01%20Song.mp3\nhttp://radio/stream\n").unwrap();
        let parsed = resolved(&f);
        assert_eq!(parsed.len(), 1);
        assert!(parsed[0].ends_with("Music\\Album\\01 Song.mp3"), "{}", parsed[0]);
        // The stream stays in the model (the import report counts it) but doesn't resolve.
        assert_eq!(parse_file(&f).unwrap().items[1].track.location.as_deref(), Some("http://radio/stream"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extinf_variants() {
        let pl = parse_str(
            "#EXTM3U\r\n#PLAYLIST:Road trip\r\n\
             #EXTINF:215 tvg-id=\"a,b\" group-title=\"x\",Daft Punk - One More Time\r\nC:\\m\\1.flac\r\n\
             #EXTINF:-1,\r\nfile://localhost/C:/m/2%20x.flac\r\n\
             /storage/emulated/0/Music/3.mp3\r\n\
             #EXTINF:200.6,Title Only\r\n#EXTART:A; B\r\n#EXTALB:Alb\r\n#1 hit.mp3\r\n.\\#1 hit.mp3\r\n",
            "stem",
        );
        assert_eq!(pl.name, "Road trip");
        let t: Vec<&TrackRef> = pl.items.iter().map(|i| &i.track).collect();
        assert_eq!(t.len(), 4, "a line starting with # is a comment, not an entry");
        assert_eq!((t[0].title.as_str(), t[0].duration_ms), ("Daft Punk - One More Time", Some(215_000)));
        assert!(t[0].artists.is_empty(), "no guessing without #EXTART");
        assert_eq!((t[1].title.as_str(), t[1].duration_ms, t[1].location.as_deref()), ("", None, Some("C:\\m\\2 x.flac")));
        assert_eq!(t[2].location.as_deref(), Some("\\storage\\emulated\\0\\Music\\3.mp3"));
        assert_eq!(t[2].title, "", "EXTINF doesn't leak into the next entry");
        assert_eq!((t[3].title.as_str(), t[3].artists.clone(), t[3].album.as_str()), ("Title Only", vec!["A; B".to_string()], "Alb"));
        assert_eq!(t[3].duration_ms, Some(200_600));
        assert_eq!(t[3].location.as_deref(), Some("#1 hit.mp3"));
    }

    #[test]
    fn utf16_and_cp1252_files() {
        let dir = std::env::temp_dir().join(format!("m3u-test3-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f16 = dir.join("u16.m3u");
        let body: Vec<u8> = [0xFF, 0xFE].into_iter().chain("#EXTM3U\r\nМало тебя.flac\r\n".encode_utf16().flat_map(|u| u.to_le_bytes())).collect();
        std::fs::write(&f16, body).unwrap();
        assert!(resolved(&f16)[0].ends_with("Мало тебя.flac"));
        let f1252 = dir.join("legacy.m3u");
        std::fs::write(&f1252, b"#EXTINF:10,Don\x92t Stop\r\nDon\x92t Stop.mp3\r\n").unwrap();
        let pl = parse_file(&f1252).unwrap();
        assert_eq!(pl.items[0].track.title, "Don’t Stop");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What M3U can carry: whole seconds, title, one artist string, album, location.
    fn project(mut pl: Playlist) -> Playlist {
        pl.source = "m3u".into();
        for it in &mut pl.items {
            let t = &mut it.track;
            let artists = t.artists.join(ARTIST_SEP);
            t.artists = if artists.is_empty() { Vec::new() } else { vec![artists] };
            t.duration_ms = t.duration_ms.map(|ms| ((ms as f64 / 1000.0).round() as u64) * 1000).filter(|ms| *ms > 0);
            t.isrc = None;
            t.mbid = None;
            t.service_ids.clear();
            it.added_at = None;
        }
        pl
    }

    proptest! {
        #[test]
        fn parse_write_roundtrip(pl in testgen::playlist("m3u")) {
            let text = write_str(&pl, None);
            prop_assert_eq!(parse_str(&text, "unused"), project(pl));
        }
    }
}
