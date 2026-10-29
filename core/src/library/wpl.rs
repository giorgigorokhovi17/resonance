//! Windows Media Player playlists (`.wpl`, SMIL-based XML).
//!
//! `<media src>` is a path, usually relative with `..\`. WMP 12 also writes `trackTitle`,
//! `trackArtist`, `albumTitle` and `duration` attributes on `<media>`. They're read when present;
//! `duration` is assumed to be milliseconds, which hasn't been checked against a real WMP file yet.

use anyhow::{bail, Result};

use super::interchange::xml;
use super::interchange::{canonical_location, Playlist, PlaylistItem, TrackRef};

pub fn parse_str(text: &str, default_name: &str) -> Result<Playlist> {
    let root = xml::parse(text)?;
    if !root.name.eq_ignore_ascii_case("smil") {
        bail!("not a WPL playlist (root element is <{}>)", root.name);
    }
    let name = root.child("head").and_then(|h| h.text_of("title")).unwrap_or_else(|| default_name.to_string());
    let mut items = Vec::new();
    if let Some(body) = root.child("body") {
        for seq in body.children_named("seq") {
            for m in seq.children_named("media") {
                let Some(location) = m.attr("src").and_then(canonical_location) else { continue };
                let attr = |k: &str| m.attr(k).map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
                items.push(PlaylistItem {
                    track: TrackRef {
                        title: attr("trackTitle").unwrap_or_default(),
                        artists: attr("trackArtist").into_iter().collect(),
                        album: attr("albumTitle").unwrap_or_default(),
                        duration_ms: attr("duration").and_then(|d| d.parse::<u64>().ok()).filter(|d| *d > 0),
                        location: Some(location),
                        ..Default::default()
                    },
                    added_at: None,
                });
            }
        }
    }
    Ok(Playlist { name, source: "wpl".into(), source_id: None, items })
}

/// Writer used by the round-trip tests. WPL isn't offered as an export format.
#[cfg(test)]
pub fn write_str(pl: &Playlist) -> String {
    use super::interchange::xml::esc;
    let mut s = String::from("<?wpl version=\"1.0\"?>\r\n<smil>\r\n  <head>\r\n");
    s.push_str(&format!("    <meta name=\"ItemCount\" content=\"{}\"/>\r\n", pl.items.len()));
    s.push_str(&format!("    <title>{}</title>\r\n  </head>\r\n  <body>\r\n    <seq>\r\n", esc(&pl.name)));
    for item in &pl.items {
        let t = &item.track;
        let Some(loc) = &t.location else { continue };
        s.push_str(&format!("      <media src=\"{}\"", esc(loc)));
        let mut attr = |k: &str, v: &str| {
            if !v.is_empty() {
                s.push_str(&format!(" {k}=\"{}\"", esc(v)));
            }
        };
        attr("trackTitle", &t.title);
        attr("trackArtist", &t.artists.join("; "));
        attr("albumTitle", &t.album);
        attr("duration", &t.duration_ms.map(|d| d.to_string()).unwrap_or_default());
        s.push_str("/>\r\n");
    }
    s.push_str("    </seq>\r\n  </body>\r\n</smil>\r\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::interchange::testgen;
    use proptest::prelude::*;

    #[test]
    fn reads_wmp_file() {
        let text = "\u{feff}<?wpl version=\"1.0\"?>\r\n<smil>\r\n<head><meta name=\"Generator\" content=\"Microsoft Windows Media Player -- 12.0.26100\"/>\
            <meta name=\"ItemCount\" content=\"2\"/><title>Rock &amp; Roll</title></head>\r\n<body><seq>\
            <media src=\"..\\Music\\Queen\\01 Don&apos;t Stop Me Now.mp3\" albumTitle=\"Jazz\" trackTitle=\"Don't Stop Me Now\" trackArtist=\"Queen\" duration=\"209000\" tid=\"{A}\" cid=\"{B}\"/>\
            <media src=\"\\\\nas\\share\\x.flac\"/>\
            <media/>\
            </seq></body></smil>";
        let pl = parse_str(text, "stem").unwrap();
        assert_eq!(pl.name, "Rock & Roll");
        assert_eq!(pl.items.len(), 2, "media without src is skipped");
        let t = &pl.items[0].track;
        assert_eq!(t.location.as_deref(), Some(r"..\Music\Queen\01 Don't Stop Me Now.mp3"));
        assert_eq!((t.title.as_str(), t.artists.clone(), t.album.as_str(), t.duration_ms), ("Don't Stop Me Now", vec!["Queen".to_string()], "Jazz", Some(209_000)));
        assert_eq!(pl.items[1].track.location.as_deref(), Some(r"\\nas\share\x.flac"));
    }

    #[test]
    fn rejects_other_xml() {
        assert!(parse_str("<playlist/>", "x").is_err());
    }

    /// What WPL can carry: title, one artist string, album, duration, location.
    fn project(mut pl: Playlist) -> Playlist {
        pl.source = "wpl".into();
        for it in &mut pl.items {
            let t = &mut it.track;
            let artists = t.artists.join("; ");
            t.artists = if artists.is_empty() { Vec::new() } else { vec![artists] };
            t.isrc = None;
            t.mbid = None;
            t.service_ids.clear();
            it.added_at = None;
        }
        pl
    }

    proptest! {
        #[test]
        fn parse_write_roundtrip(pl in testgen::playlist("wpl")) {
            prop_assert_eq!(parse_str(&write_str(&pl), "unused").unwrap(), project(pl));
        }
    }
}
