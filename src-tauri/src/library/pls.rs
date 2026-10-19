//! PLS playlists (`[playlist]`, `File<n>=`, `Title<n>=`, `Length<n>=`).
//!
//! Keys are case-insensitive. Entries are ordered by their number, not by line order, and gaps in
//! the numbering are allowed. `NumberOfEntries` is ignored because writers often get it wrong.
//! PLS has no artist or album field: `Title` is kept whole, as the M3U display string is.

use std::collections::BTreeMap;

use anyhow::{bail, Result};

use super::interchange::{canonical_location, Playlist, PlaylistItem, TrackRef};

pub fn parse_str(text: &str, default_name: &str) -> Result<Playlist> {
    #[derive(Default)]
    struct Entry {
        file: Option<String>,
        title: Option<String>,
        length: Option<i64>,
    }
    let mut entries: BTreeMap<u32, Entry> = BTreeMap::new();
    let mut name = None;
    let mut saw_header = false;
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            saw_header |= line.eq_ignore_ascii_case("[playlist]");
            continue;
        }
        let Some((key, val)) = line.split_once('=') else { continue };
        let key = key.trim().to_ascii_lowercase();
        let val = val.trim();
        let (field, digits) = key.split_at(key.trim_end_matches(|c: char| c.is_ascii_digit()).len());
        if key == "x-playlistname" || key == "playlistname" {
            if !val.is_empty() {
                name = Some(val.to_string());
            }
            continue;
        }
        let Ok(n) = digits.parse::<u32>() else { continue };
        let e = entries.entry(n).or_default();
        match field {
            "file" => e.file = Some(val.to_string()),
            "title" => e.title = Some(val.to_string()),
            "length" => e.length = val.parse().ok(),
            _ => {}
        }
    }
    if !saw_header && entries.is_empty() {
        bail!("not a PLS playlist (no [playlist] section)");
    }
    let items = entries
        .into_values()
        .filter_map(|e| {
            let location = canonical_location(e.file.as_deref()?)?;
            Some(PlaylistItem {
                track: TrackRef {
                    title: e.title.unwrap_or_default(),
                    duration_ms: e.length.filter(|s| *s > 0).map(|s| s as u64 * 1000),
                    location: Some(location),
                    ..Default::default()
                },
                added_at: None,
            })
        })
        .collect();
    Ok(Playlist { name: name.unwrap_or_else(|| default_name.to_string()), source: "pls".into(), source_id: None, items })
}

/// Writer used by the round-trip tests. PLS isn't offered as an export format.
#[cfg(test)]
pub fn write_str(pl: &Playlist) -> String {
    let mut s = String::from("[playlist]\r\n");
    s.push_str(&format!("X-PlaylistName={}\r\n", pl.name));
    let mut n = 0;
    for item in &pl.items {
        let t = &item.track;
        let Some(loc) = &t.location else { continue };
        n += 1;
        let artists = t.artists.join("; ");
        let title = if artists.is_empty() { t.title.clone() } else { format!("{artists} - {}", t.title) };
        let secs = t.duration_ms.map(|ms| (ms as f64 / 1000.0).round() as i64).filter(|s| *s > 0).unwrap_or(-1);
        s.push_str(&format!("File{n}={loc}\r\nTitle{n}={title}\r\nLength{n}={secs}\r\n"));
    }
    s.push_str(&format!("NumberOfEntries={n}\r\nVersion=2\r\n"));
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::interchange::testgen;
    use proptest::prelude::*;

    #[test]
    fn out_of_order_gaps_and_case() {
        let pl = parse_str(
            "[Playlist]\nnumberofentries=9\nFILE3=C:\\b.mp3\ntitle3=B\nFile1=http://radio/x\nTitle1=Radio\nLength1=-1\n\
             File2=..\\a.flac\nLength2=180\n; comment\nVersion=2\n",
            "stem",
        )
        .unwrap();
        assert_eq!(pl.name, "stem");
        let locs: Vec<_> = pl.items.iter().map(|i| i.track.location.clone().unwrap()).collect();
        assert_eq!(locs, ["http://radio/x", "..\\a.flac", "C:\\b.mp3"]);
        assert_eq!(pl.items[0].track.duration_ms, None);
        assert_eq!(pl.items[1].track.duration_ms, Some(180_000));
        assert_eq!(pl.items[2].track.title, "B");
    }

    #[test]
    fn rejects_non_pls() {
        assert!(parse_str("#EXTM3U\nC:\\a.mp3\n", "x").is_err());
    }

    /// What PLS can carry: whole seconds, one display title ("artists - title"), location.
    fn project(mut pl: Playlist) -> Playlist {
        pl.source = "pls".into();
        for it in &mut pl.items {
            let t = &mut it.track;
            let artists = t.artists.join("; ");
            if !artists.is_empty() {
                t.title = format!("{artists} - {}", t.title);
            }
            *t = TrackRef {
                title: std::mem::take(&mut t.title),
                duration_ms: t.duration_ms.map(|ms| ((ms as f64 / 1000.0).round() as u64) * 1000).filter(|ms| *ms > 0),
                location: t.location.take(),
                ..Default::default()
            };
            it.added_at = None;
        }
        pl
    }

    proptest! {
        #[test]
        fn parse_write_roundtrip(pl in testgen::playlist("pls")) {
            prop_assert_eq!(parse_str(&write_str(&pl), "unused").unwrap(), project(pl));
        }
    }
}
