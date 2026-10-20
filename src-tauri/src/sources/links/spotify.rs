//! Spotify: the public embed page gives the playlist's name, its first ~50-100 tracks and an
//! anonymous access token. With that token, Spotify's own web-player API (`pathfinder`,
//! persisted query `fetchPlaylist`) pages through the rest, 100 at a time. Same approach as
//! Riff; unlike Riff, the track's own artists are used (not the album's), so features match.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

use super::{script_with, send, Fetched};
use crate::library::csv::parse_rfc3339;
use crate::library::interchange::{Playlist, PlaylistItem, TrackRef};

const PATHFINDER: &str = "https://api-partner.spotify.com/pathfinder/v1/query";
/// Persisted-query hash of the web player's `fetchPlaylist` operation (from Riff, MIT).
const FETCH_PLAYLIST_HASH: &str = "8964e8eafb21aa992a7d951d256d83285c04be2105d209262901de70cb97584a";
const PAGE: u64 = 100;
const MAX_PAGES: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Playlist,
    Album,
}

/// What the embed page says.
#[derive(Debug, Default)]
pub struct Embed {
    pub name: String,
    pub token: Option<String>,
    pub items: Vec<PlaylistItem>,
}

fn track_id(uri: &str) -> Option<String> {
    uri.strip_prefix("spotify:track:").map(str::to_string)
}

fn spotify_ref(title: &str, artists: Vec<String>, album: String, ms: Option<u64>, uri: Option<&str>) -> TrackRef {
    let mut t = TrackRef { title: title.trim().to_string(), artists, album, duration_ms: ms.filter(|d| *d > 0), ..Default::default() };
    if let Some(id) = uri.and_then(track_id) {
        t.service_ids.insert("spotify".into(), id);
    }
    t
}

/// Parse the embed page (`__NEXT_DATA__` JSON). Artists come as one "A, B" subtitle.
pub fn parse_embed(html: &str) -> Result<Embed> {
    let raw = script_with(html, "__NEXT_DATA__").ok_or_else(|| anyhow!("Spotify's page has changed (no playlist data found)."))?;
    let v: Value = serde_json::from_str(&raw).context("Spotify's page data isn't valid JSON")?;
    let state = &v["props"]["pageProps"]["state"];
    let entity = &state["data"]["entity"];
    if entity.is_null() {
        bail!("Spotify's page has changed (no playlist in its data).");
    }
    let name = entity["name"].as_str().or(entity["title"].as_str()).unwrap_or("Spotify playlist").trim().to_string();
    let token = state["settings"]["session"]["accessToken"].as_str().filter(|t| !t.is_empty()).map(str::to_string);
    let items = entity["trackList"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter(|t| t["entityType"].as_str().is_none_or(|k| k == "track"))
                .filter_map(|t| {
                    let title = t["title"].as_str().filter(|s| !s.trim().is_empty())?;
                    let artists = t["subtitle"].as_str().unwrap_or("").split(',').map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect();
                    let r = spotify_ref(title, artists, String::new(), t["duration"].as_u64(), t["uri"].as_str());
                    Some(PlaylistItem { track: r, added_at: None })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Embed { name, token, items })
}

/// One `fetchPlaylist` page: (total tracks, items). Episodes and unavailable entries are skipped.
pub fn parse_page(v: &Value) -> Result<(u64, Vec<PlaylistItem>)> {
    let content = &v["data"]["playlistV2"]["content"];
    let Some(list) = content["items"].as_array() else {
        if let Some(msg) = v["errors"][0]["message"].as_str() {
            bail!("Spotify: {msg}");
        }
        bail!("Spotify's playlist API has changed (no items).");
    };
    let total = content["totalCount"].as_u64().unwrap_or(list.len() as u64);
    let items = list
        .iter()
        .filter_map(|it| {
            let d = if it["itemV2"]["data"].is_object() { &it["itemV2"]["data"] } else { &it["item"]["data"] };
            if d["__typename"].as_str().is_some_and(|t| t != "Track") {
                return None;
            }
            let title = d["name"].as_str().filter(|s| !s.trim().is_empty())?;
            let names = |arr: &Value| -> Vec<String> {
                arr["items"].as_array().map(|a| a.iter().filter_map(|x| x["profile"]["name"].as_str().map(str::to_string)).collect()).unwrap_or_default()
            };
            let mut artists = names(&d["artists"]);
            if artists.is_empty() {
                artists = names(&d["albumOfTrack"]["artists"]);
            }
            let album = d["albumOfTrack"]["name"].as_str().unwrap_or("").to_string();
            let ms = d["trackDuration"]["totalMilliseconds"].as_u64();
            let r = spotify_ref(title, artists, album, ms, d["uri"].as_str());
            let added_at = it["addedAt"]["isoString"].as_str().and_then(parse_rfc3339);
            Some(PlaylistItem { track: r, added_at })
        })
        .collect();
    Ok((total, items))
}

async fn pathfinder_page(http: &reqwest::Client, token: &str, id: &str, offset: u64) -> Result<Value> {
    let body = json!({
        "variables": { "uri": format!("spotify:playlist:{id}"), "offset": offset, "limit": PAGE, "enableWatchFeedEntrypoint": true },
        "operationName": "fetchPlaylist",
        "extensions": { "persistedQuery": { "version": 1, "sha256Hash": FETCH_PLAYLIST_HASH } }
    });
    let req = http
        .post(PATHFINDER)
        .bearer_auth(token)
        .header("Referer", "https://open.spotify.com/")
        .header("Accept-Language", "en-US,en;q=0.9")
        .json(&body);
    Ok(send(req, "Spotify").await?.json::<Value>().await?)
}

pub async fn fetch(http: &reqwest::Client, kind: Kind, id: &str) -> Result<Fetched> {
    let what = if kind == Kind::Album { "album" } else { "playlist" };
    let html = send(http.get(format!("https://open.spotify.com/embed/{what}/{id}")).header("Accept-Language", "en-US,en;q=0.9"), "Spotify")
        .await?
        .text()
        .await?;
    let embed = parse_embed(&html)?;
    let mut note = None;
    let mut items = embed.items;
    if kind == Kind::Playlist {
        match embed.token.as_deref() {
            Some(token) => match fetch_all(http, token, id).await {
                Ok(all) if all.len() >= items.len() => items = all,
                Ok(_) => {}
                Err(e) => {
                    log::warn!("spotify paging failed: {e:#}");
                    note = Some(format!("Spotify only gave the first {} songs: {e}", items.len()));
                }
            },
            None => note = Some(format!("Spotify only gave the first {} songs.", items.len())),
        }
    }
    if items.is_empty() {
        bail!("This Spotify {what} has no songs, or isn't public.");
    }
    let playlist = Playlist { name: embed.name, source: "spotify".into(), source_id: Some(format!("{what}:{id}")), items };
    Ok(Fetched { playlist, note })
}

async fn fetch_all(http: &reqwest::Client, token: &str, id: &str) -> Result<Vec<PlaylistItem>> {
    let (total, mut items) = parse_page(&pathfinder_page(http, token, id, 0).await?)?;
    let mut offset = items.len() as u64;
    // Raw entries per page, including skipped episodes, decide the next offset.
    let mut pages = 1;
    while offset < total && pages < MAX_PAGES {
        let v = pathfinder_page(http, token, id, offset).await?;
        let raw = v["data"]["playlistV2"]["content"]["items"].as_array().map(|a| a.len() as u64).unwrap_or(0);
        if raw == 0 {
            break;
        }
        let (_, page) = parse_page(&v)?;
        items.extend(page);
        offset += raw;
        pages += 1;
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn embed_html(entity: &str) -> String {
        format!(
            r#"<html><script id="__NEXT_DATA__" type="application/json">{{"props":{{"pageProps":{{"state":{{"data":{{"entity":{entity}}},"settings":{{"session":{{"accessToken":"tok","isAnonymous":true}}}}}}}}}}}}</script></html>"#
        )
    }

    #[test]
    fn embed_page() {
        let html = embed_html(
            r#"{"type":"playlist","name":"Mix","trackList":[
                {"uri":"spotify:track:AAA","title":"expectations (feat. Chappell Roan)","subtitle":"Olivia Rodrigo, Chappell Roan","duration":223935,"entityType":"track"},
                {"uri":"spotify:episode:EEE","title":"A podcast","subtitle":"Show","duration":1,"entityType":"episode"},
                {"uri":"spotify:track:BBB","title":"  ","subtitle":"x","duration":1,"entityType":"track"}]}"#,
        );
        let e = parse_embed(&html).unwrap();
        assert_eq!((e.name.as_str(), e.token.as_deref(), e.items.len()), ("Mix", Some("tok"), 1));
        let t = &e.items[0].track;
        assert_eq!(t.artists, vec!["Olivia Rodrigo".to_string(), "Chappell Roan".to_string()], "nbsp after the comma is trimmed");
        assert_eq!((t.duration_ms, t.service_ids.get("spotify").map(String::as_str)), (Some(223_935), Some("AAA")));
        assert!(parse_embed("<html>changed</html>").is_err());
    }

    #[test]
    fn api_page() {
        let v: Value = serde_json::from_str(
            r#"{"data":{"playlistV2":{"content":{"totalCount":150,"items":[
                {"addedAt":{"isoString":"2026-04-15T22:42:54.768Z"},"itemV2":{"data":{"__typename":"Track","name":"Beauty And A Beat","uri":"spotify:track:190j",
                  "artists":{"items":[{"profile":{"name":"Justin Bieber"}},{"profile":{"name":"Nicki Minaj"}}]},
                  "albumOfTrack":{"name":"Believe","artists":{"items":[{"profile":{"name":"Justin Bieber"}}]}},
                  "trackDuration":{"totalMilliseconds":227986}}}},
                {"itemV2":{"data":{"__typename":"Episode","name":"Pod"}}},
                {"itemV2":{"data":{"__typename":"Track","name":"Old shape","albumOfTrack":{"name":"A","artists":{"items":[{"profile":{"name":"Album Artist"}}]}}}}}
            ]}}}}"#,
        )
        .unwrap();
        let (total, items) = parse_page(&v).unwrap();
        assert_eq!((total, items.len()), (150, 2));
        let t = &items[0].track;
        assert_eq!(t.artists, vec!["Justin Bieber".to_string(), "Nicki Minaj".to_string()], "track artists, not the album's");
        assert_eq!((t.album.as_str(), t.duration_ms), ("Believe", Some(227_986)));
        assert_eq!(items[0].added_at, Some(1_776_292_974));
        assert_eq!(items[1].track.artists, vec!["Album Artist".to_string()], "falls back to album artists");
        let err: Value = serde_json::from_str(r#"{"errors":[{"message":"PersistedQueryNotFound"}]}"#).unwrap();
        assert!(parse_page(&err).unwrap_err().to_string().contains("PersistedQueryNotFound"));
    }
}
