//! Apple Music: albums through Apple's public iTunes lookup API; playlists from the page's
//! embedded `serialized-server-data` JSON (falling back to its schema.org JSON-LD). Apple's page
//! only carries the first 100 tracks of a long playlist.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;

use super::{script_with, send, Fetched};
use crate::library::interchange::{Playlist, PlaylistItem, TrackRef};

fn apple_ref(title: &str, artists: Vec<String>, album: String, ms: Option<u64>, id: Option<String>) -> TrackRef {
    let mut t = TrackRef { title: title.trim().to_string(), artists, album, duration_ms: ms.filter(|d| *d > 0), ..Default::default() };
    if let Some(id) = id {
        t.service_ids.insert("apple".into(), id);
    }
    t
}

/// ISO 8601 duration like "PT3M43S" in milliseconds.
pub fn iso_duration_ms(s: &str) -> Option<u64> {
    let rest = s.strip_prefix("PT")?;
    let (mut total, mut num) = (0f64, String::new());
    for c in rest.chars() {
        match c {
            '0'..='9' | '.' => num.push(c),
            'H' | 'M' | 'S' => {
                let n: f64 = num.parse().ok()?;
                total += n * match c {
                    'H' => 3600.0,
                    'M' => 60.0,
                    _ => 1.0,
                };
                num.clear();
            }
            _ => return None,
        }
    }
    Some((total * 1000.0).round() as u64)
}

/// Parse a playlist page.
pub fn parse_playlist_page(html: &str) -> Result<(String, Vec<PlaylistItem>)> {
    if let Some(raw) = script_with(html, r#"id="serialized-server-data""#) {
        let v: Value = serde_json::from_str(&raw).context("Apple Music's page data isn't valid JSON")?;
        let mut name = None;
        let mut items: Vec<PlaylistItem> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        fn walk<'a>(v: &'a Value, name: &mut Option<String>, out: &mut Vec<&'a Value>) {
            match v {
                Value::Object(m) => {
                    if name.is_none() && m.contains_key("trackCount") {
                        *name = m.get("title").and_then(|t| t.as_str()).map(str::to_string);
                    }
                    if m.contains_key("duration") && m.contains_key("title") && (m.contains_key("subtitleLinks") || m.contains_key("artistName")) {
                        out.push(v);
                    }
                    m.values().for_each(|c| walk(c, name, out));
                }
                Value::Array(a) => a.iter().for_each(|c| walk(c, name, out)),
                _ => {}
            }
        }
        let mut songs = Vec::new();
        walk(&v, &mut name, &mut songs);
        for s in songs {
            let Some(title) = s["title"].as_str().filter(|t| !t.trim().is_empty()) else { continue };
            let id = s["contentDescriptor"]["identifiers"]["storeAdamID"].as_str().map(str::to_string);
            if let Some(id) = &id {
                if !seen.insert(id.clone()) {
                    continue;
                }
            }
            let mut artists: Vec<String> =
                s["subtitleLinks"].as_array().map(|a| a.iter().filter_map(|l| l["title"].as_str().map(str::to_string)).collect()).unwrap_or_default();
            if artists.is_empty() {
                artists.extend(s["artistName"].as_str().map(str::to_string));
            }
            let album = s["tertiaryLinks"][0]["title"].as_str().unwrap_or("").to_string();
            items.push(PlaylistItem { track: apple_ref(title, artists, album, s["duration"].as_u64(), id), added_at: None });
        }
        if !items.is_empty() {
            return Ok((name.unwrap_or_else(|| "Apple Music playlist".into()), items));
        }
    }
    // Fallback: schema.org data (no artists per track, but names and durations).
    let raw = script_with(html, "schema:music-playlist").ok_or_else(|| anyhow!("Apple Music's page has changed (no playlist data found)."))?;
    let v: Value = serde_json::from_str(&raw).context("Apple Music's playlist data isn't valid JSON")?;
    let items = v["track"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    let title = t["name"].as_str()?;
                    let ms = t["duration"].as_str().and_then(iso_duration_ms);
                    Some(PlaylistItem { track: apple_ref(title, Vec::new(), String::new(), ms, None), added_at: None })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok((v["name"].as_str().unwrap_or("Apple Music playlist").to_string(), items))
}

/// Parse an iTunes `lookup?entity=song` answer for an album.
pub fn parse_album_lookup(v: &Value) -> Result<(String, Vec<PlaylistItem>)> {
    let results = v["results"].as_array().ok_or_else(|| anyhow!("Apple's lookup API has changed."))?;
    let name = results.iter().find(|r| r["wrapperType"] == "collection").and_then(|r| r["collectionName"].as_str()).unwrap_or("Apple Music album");
    let items = results
        .iter()
        .filter(|r| r["wrapperType"] == "track")
        .filter_map(|r| {
            let title = r["trackName"].as_str()?;
            let artists = r["artistName"].as_str().map(|a| vec![a.to_string()]).unwrap_or_default();
            let album = r["collectionName"].as_str().unwrap_or("").to_string();
            let id = r["trackId"].as_u64().map(|i| i.to_string());
            Some(PlaylistItem { track: apple_ref(title, artists, album, r["trackTimeMillis"].as_u64(), id), added_at: None })
        })
        .collect();
    Ok((name.to_string(), items))
}

pub async fn fetch_playlist(http: &reqwest::Client, url: &str, id: &str) -> Result<Fetched> {
    let html = send(http.get(url).header("Accept-Language", "en-US,en;q=0.9"), "Apple Music").await?.text().await?;
    let (name, items) = parse_playlist_page(&html)?;
    if items.is_empty() {
        bail!("This Apple Music playlist has no songs, or isn't public.");
    }
    let note = (items.len() >= 100).then(|| "Apple Music's page only lists the first 100 songs of a playlist.".to_string());
    Ok(Fetched { playlist: Playlist { name, source: "apple-music".into(), source_id: Some(id.to_string()), items }, note })
}

pub async fn fetch_album(http: &reqwest::Client, id: &str) -> Result<Fetched> {
    let v: Value = send(http.get(format!("https://itunes.apple.com/lookup?id={id}&entity=song")), "Apple Music").await?.json().await?;
    let (name, items) = parse_album_lookup(&v)?;
    if items.is_empty() {
        bail!("Apple Music has no songs for this album link.");
    }
    Ok(Fetched { playlist: Playlist { name, source: "apple-music".into(), source_id: Some(format!("album:{id}")), items }, note: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(iso_duration_ms("PT3M43S"), Some(223_000));
        assert_eq!(iso_duration_ms("PT1H2M3S"), Some(3_723_000));
        assert_eq!(iso_duration_ms("3:43"), None);
    }

    #[test]
    fn playlist_page() {
        let html = r#"<script type="application/json" id="serialized-server-data">{"data":[{"sections":[
            {"items":[{"title":"Today’s Hits","trackCount":50}]},
            {"items":[
              {"id":"t1","title":"expectations","duration":223935,"artistName":"Olivia Rodrigo & Chappell Roan",
               "subtitleLinks":[{"title":"Olivia Rodrigo"},{"title":"Chappell Roan"}],
               "tertiaryLinks":[{"title":"expectations - Single"}],
               "contentDescriptor":{"identifiers":{"storeAdamID":"6822390426"}}},
              {"id":"t1-dup","title":"expectations","duration":223935,"artistName":"x","contentDescriptor":{"identifiers":{"storeAdamID":"6822390426"}}}
            ]}]}]}</script>"#;
        let (name, items) = parse_playlist_page(html).unwrap();
        assert_eq!((name.as_str(), items.len()), ("Today’s Hits", 1), "duplicates by Apple id dropped");
        let t = &items[0].track;
        assert_eq!(t.artists, vec!["Olivia Rodrigo".to_string(), "Chappell Roan".to_string()]);
        assert_eq!((t.album.as_str(), t.duration_ms), ("expectations - Single", Some(223_935)));
        assert!(parse_playlist_page("<html></html>").is_err());
    }

    #[test]
    fn album_lookup() {
        let v: Value = serde_json::from_str(
            r#"{"results":[{"wrapperType":"collection","collectionName":"good kid, m.A.A.d city"},
            {"wrapperType":"track","trackName":"Swimming Pools (Drank) [Extended Version]","artistName":"Kendrick Lamar","collectionName":"good kid, m.A.A.d city","trackTimeMillis":313787,"trackId":1440818890}]}"#,
        )
        .unwrap();
        let (name, items) = parse_album_lookup(&v).unwrap();
        assert_eq!((name.as_str(), items.len(), items[0].track.duration_ms), ("good kid, m.A.A.d city", 1, Some(313_787)));
    }
}
