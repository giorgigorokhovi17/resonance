//! XSPF playlists (https://www.xspf.org/spec).
//!
//! `location` is a URI. File URIs and relative URIs are decoded to paths. `duration` is in
//! milliseconds. `creator` is a single string, kept as one artist.
//!
//! XSPF has no ISRC field or multi-artist list. The writer keeps them in `<meta>` with
//! `urn:x-resonance:*` rel URIs, which other players ignore. The MusicBrainz recording id goes in
//! `<identifier>`, the way MusicBrainz tools write it.

use std::path::Path;

use anyhow::{bail, Context, Result};

use super::interchange::xml::{self, esc};
use super::interchange::{
    is_url, location_to_uri, mbid_from_uri, normalize_isrc, relative_to, service_id_from_uri, uri_to_location, Playlist,
    PlaylistItem, TrackRef,
};

const META_ARTIST: &str = "urn:x-resonance:artist";
const META_ISRC: &str = "urn:x-resonance:isrc";
const META_ADDED: &str = "urn:x-resonance:added-at";
const META_SERVICE: &str = "urn:x-resonance:service-id:";

fn isrc_from_identifier(id: &str) -> Option<String> {
    let lower = id.to_ascii_lowercase();
    let rest = lower.strip_prefix("urn:isrc:").or_else(|| lower.strip_prefix("isrc:"))?;
    normalize_isrc(rest)
}

pub fn parse_str(text: &str, default_name: &str) -> Result<Playlist> {
    let root = xml::parse(text)?;
    if !root.name.eq_ignore_ascii_case("playlist") {
        bail!("not an XSPF playlist (root element is <{}>)", root.name);
    }
    let mut pl = Playlist {
        name: root.text_of("title").unwrap_or_else(|| default_name.to_string()),
        source: "xspf".into(),
        source_id: root.text_of("identifier"),
        items: Vec::new(),
    };
    let tracks = root.child("trackList").map(|l| l.children_named("track").collect::<Vec<_>>()).unwrap_or_default();
    for tr in tracks {
        let mut t = TrackRef {
            title: tr.text_of("title").unwrap_or_default(),
            album: tr.text_of("album").unwrap_or_default(),
            duration_ms: tr.text_of("duration").and_then(|d| d.parse::<f64>().ok()).filter(|d| *d > 0.0).map(|d| d.round() as u64),
            location: tr.text_of("location").and_then(|l| uri_to_location(&l)),
            ..Default::default()
        };
        let mut added_at = None;
        let mut meta_artists = Vec::new();
        for m in tr.children_named("meta") {
            let rel = m.attr("rel").unwrap_or("");
            let val = m.text.trim();
            if val.is_empty() {
                continue;
            }
            if rel == META_ARTIST {
                meta_artists.push(val.to_string());
            } else if rel == META_ISRC {
                t.isrc = t.isrc.take().or_else(|| normalize_isrc(val));
            } else if rel == META_ADDED {
                added_at = val.parse().ok();
            } else if let Some(service) = rel.strip_prefix(META_SERVICE) {
                t.service_ids.insert(service.to_string(), val.to_string());
            }
        }
        t.artists = if meta_artists.is_empty() { tr.text_of("creator").into_iter().collect() } else { meta_artists };
        let uris = tr.children_named("identifier").chain(tr.children_named("location")).map(|e| e.text.trim().to_string());
        for id in uris {
            if t.mbid.is_none() {
                t.mbid = mbid_from_uri(&id);
            }
            if t.isrc.is_none() {
                t.isrc = isrc_from_identifier(&id);
            }
            if let Some((service, sid)) = service_id_from_uri(&id) {
                t.service_ids.entry(service.to_string()).or_insert(sid);
            }
        }
        pl.items.push(PlaylistItem { track: t, added_at });
    }
    Ok(pl)
}

/// XSPF text. Paths inside `base` (the playlist's folder) become relative URIs.
pub fn write_str(pl: &Playlist, base: Option<&Path>) -> String {
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<playlist version=\"1\" xmlns=\"http://xspf.org/ns/0/\">\n");
    let el = |s: &mut String, indent: &str, name: &str, val: &str| {
        s.push_str(&format!("{indent}<{name}>{}</{name}>\n", esc(val)));
    };
    if !pl.name.is_empty() {
        el(&mut s, "  ", "title", &pl.name);
    }
    if let Some(id) = pl.source_id.as_deref().filter(|id| is_url(id)) {
        el(&mut s, "  ", "identifier", id);
    }
    s.push_str("  <trackList>\n");
    for item in &pl.items {
        let t = &item.track;
        let ind = "      ";
        s.push_str("    <track>\n");
        if let Some(loc) = &t.location {
            el(&mut s, ind, "location", &location_to_uri(&relative_to(loc, base)));
        }
        if let Some(mbid) = &t.mbid {
            el(&mut s, ind, "identifier", &format!("https://musicbrainz.org/recording/{mbid}"));
        }
        if !t.title.is_empty() {
            el(&mut s, ind, "title", &t.title);
        }
        if !t.artists.is_empty() {
            el(&mut s, ind, "creator", &t.artists.join("; "));
        }
        if !t.album.is_empty() {
            el(&mut s, ind, "album", &t.album);
        }
        if let Some(ms) = t.duration_ms {
            el(&mut s, ind, "duration", &ms.to_string());
        }
        let meta = |s: &mut String, rel: &str, val: &str| {
            s.push_str(&format!("{ind}<meta rel=\"{}\">{}</meta>\n", esc(rel), esc(val)));
        };
        for a in &t.artists {
            meta(&mut s, META_ARTIST, a);
        }
        if let Some(isrc) = &t.isrc {
            meta(&mut s, META_ISRC, isrc);
        }
        if let Some(at) = item.added_at {
            meta(&mut s, META_ADDED, &at.to_string());
        }
        for (service, id) in &t.service_ids {
            meta(&mut s, &format!("{META_SERVICE}{service}"), id);
        }
        s.push_str("    </track>\n");
    }
    s.push_str("  </trackList>\n</playlist>\n");
    s
}

pub fn write_file(file: &Path, pl: &Playlist) -> Result<()> {
    std::fs::write(file, write_str(pl, file.parent())).with_context(|| format!("cannot write {}", file.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::interchange::testgen;
    use proptest::prelude::*;

    #[test]
    fn reads_vlc_style_file() {
        let text = r#"<?xml version="1.0" encoding="UTF-8"?>
<playlist xmlns="http://xspf.org/ns/0/" xmlns:vlc="http://www.videolan.org/vlc/playlist/ns/0/" version="1">
	<title>Lista &amp; más</title>
	<trackList>
		<track>
			<location>file:///C:/Users/G/Music/AC-DC%20-%20T.N.T..flac</location>
			<title>T.N.T. &lt;live&gt;</title>
			<creator>AC/DC</creator>
			<duration>214733</duration>
			<extension application="http://www.videolan.org/vlc/playlist/0">
				<vlc:id>0</vlc:id>
			</extension>
		</track>
		<track>
			<location>../Música/%D0%9C%D0%B0%D0%BB%D0%BE.opus</location>
			<identifier>https://musicbrainz.org/recording/a98c6c17-5415-40f5-b9bc-ee537c2460ff</identifier>
			<identifier>isrc:USHR11536621</identifier>
		</track>
		<track>
			<location>https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC</location>
			<title><![CDATA[Never <Gonna>]]></title>
		</track>
	</trackList>
	<extension application="http://www.videolan.org/vlc/playlist/0"><vlc:item tid="0"/></extension>
</playlist>"#;
        let pl = parse_str(text, "stem").unwrap();
        assert_eq!(pl.name, "Lista & más");
        let t: Vec<&TrackRef> = pl.items.iter().map(|i| &i.track).collect();
        assert_eq!(t.len(), 3);
        assert_eq!(t[0].location.as_deref(), Some(r"C:\Users\G\Music\AC-DC - T.N.T..flac"));
        assert_eq!((t[0].title.as_str(), t[0].artists.clone(), t[0].duration_ms), ("T.N.T. <live>", vec!["AC/DC".to_string()], Some(214_733)));
        assert_eq!(t[1].location.as_deref(), Some(r"..\Música\Мало.opus"));
        assert_eq!(t[1].mbid.as_deref(), Some("a98c6c17-5415-40f5-b9bc-ee537c2460ff"));
        assert_eq!(t[1].isrc.as_deref(), Some("USHR11536621"));
        assert_eq!(t[2].service_ids.get("spotify").map(String::as_str), Some("4uLU6hMCjMI75M1A2tKUQC"));
        assert_eq!(t[2].title, "Never <Gonna>");
    }

    #[test]
    fn rejects_other_xml() {
        assert!(parse_str("<smil><body/></smil>", "x").is_err());
        assert!(parse_str("not xml <", "x").is_err());
    }

    #[test]
    fn writes_valid_absolute_and_relative_uris() {
        let pl = Playlist {
            name: "N".into(),
            items: vec![
                PlaylistItem { track: TrackRef { title: "A & B".into(), location: Some(r"C:\m\sub\a #1.flac".into()), ..Default::default() }, added_at: None },
                PlaylistItem { track: TrackRef { location: Some(r"D:\x.flac".into()), ..Default::default() }, added_at: None },
            ],
            ..Default::default()
        };
        let s = write_str(&pl, Some(Path::new(r"C:\m")));
        assert!(s.contains("<location>sub/a%20%231.flac</location>"), "{s}");
        assert!(s.contains("<location>file:///D:/x.flac</location>"), "{s}");
        assert!(s.contains("<title>A &amp; B</title>"));
    }

    proptest! {
        /// XSPF carries the whole model except `source_id` for non-URI ids.
        #[test]
        fn parse_write_roundtrip(pl in testgen::playlist("xspf")) {
            prop_assert_eq!(parse_str(&write_str(&pl, None), "unused").unwrap(), pl);
        }
    }
}
