//! YouTube and YouTube Music playlists, through the web player's own `browse` endpoint (the
//! same data youtube.com loads; no key, no sign-in). Riff shells out to yt-dlp for this; calling
//! the endpoint directly gives the same tracklist without shipping a separate program.
//!
//! YouTube serves entries in two shapes: the newer `lockupViewModel` and the classic
//! `playlistVideoRenderer`. Both are read. Video titles are kept whole ("Artist - Title
//! (Official Video)"); the matcher already splits and cleans those.

use anyhow::{bail, Result};
use serde_json::{json, Value};

use super::{clock_ms, collect_key, send, Fetched};
use crate::library::interchange::{Playlist, PlaylistItem, TrackRef};

const BROWSE: &str = "https://www.youtube.com/youtubei/v1/browse?prettyPrint=false";
const CLIENT_VERSION: &str = "2.20251001.00.00";
const MAX_PAGES: usize = 60;

/// One page: playlist title (first page only), entries, continuation token.
#[derive(Debug, Default)]
pub struct Page {
    pub title: Option<String>,
    pub items: Vec<PlaylistItem>,
    pub continuation: Option<String>,
}

/// "Kendrick Lamar - Topic" → "Kendrick Lamar"; "Shakira and 2 more" → "Shakira".
pub fn clean_channel(s: &str) -> String {
    let mut c = s.trim();
    if let Some(x) = c.strip_suffix(" - Topic") {
        c = x;
    }
    if let Some(i) = c.rfind(" and ") {
        let tail = &c[i + 5..];
        if tail.ends_with(" more") && tail.split(' ').next().is_some_and(|n| n.chars().all(|ch| ch.is_ascii_digit())) {
            c = &c[..i];
        }
    }
    c.trim().to_string()
}

fn text(v: &Value) -> Option<String> {
    if let Some(s) = v["simpleText"].as_str() {
        return Some(s.to_string());
    }
    if let Some(s) = v["content"].as_str() {
        return Some(s.to_string());
    }
    let runs = v["runs"].as_array()?;
    Some(runs.iter().filter_map(|r| r["text"].as_str()).collect::<String>())
}

fn entry(video_id: &str, title: &str, channel: Option<String>, ms: Option<u64>) -> Option<PlaylistItem> {
    let title = title.trim();
    if title.is_empty() || title == "[Private video]" || title == "[Deleted video]" {
        return None;
    }
    let mut t = TrackRef {
        title: title.to_string(),
        artists: channel.map(|c| clean_channel(&c)).filter(|c| !c.is_empty()).into_iter().collect(),
        duration_ms: ms,
        ..Default::default()
    };
    t.service_ids.insert("youtube".into(), video_id.to_string());
    Some(PlaylistItem { track: t, added_at: None })
}

fn from_lockup(v: &Value) -> Option<PlaylistItem> {
    if v["contentType"].as_str().is_some_and(|t| t != "LOCKUP_CONTENT_TYPE_VIDEO") {
        return None;
    }
    let id = v["contentId"].as_str()?;
    let meta = &v["metadata"]["lockupMetadataViewModel"];
    let title = meta["title"]["content"].as_str()?;
    let channel = meta["metadata"]["contentMetadataViewModel"]["metadataRows"][0]["metadataParts"][0]["text"]["content"]
        .as_str()
        .map(str::to_string);
    let mut badges = Vec::new();
    collect_key(&v["contentImage"], "thumbnailBadgeViewModel", &mut badges);
    let ms = badges.iter().find_map(|b| b["text"].as_str().and_then(clock_ms));
    entry(id, title, channel, ms)
}

fn from_renderer(v: &Value) -> Option<PlaylistItem> {
    let id = v["videoId"].as_str()?;
    let title = text(&v["title"])?;
    let channel = text(&v["shortBylineText"]);
    let ms = v["lengthSeconds"].as_str().and_then(|s| s.parse::<u64>().ok()).map(|s| s * 1000).or_else(|| text(&v["lengthText"]).and_then(|t| clock_ms(&t)));
    entry(id, &title, channel, ms)
}

/// Parse a `browse` response (first page or continuation).
pub fn parse_page(v: &Value) -> Result<Page> {
    // A private or missing playlist comes back as an alert with no contents.
    if v["contents"].is_null() && v["onResponseReceivedActions"].is_null() {
        if let Some(msg) = text(&v["alerts"][0]["alertRenderer"]["text"]) {
            bail!("YouTube: {msg}");
        }
    }
    let mut page = Page {
        title: v["metadata"]["playlistMetadataRenderer"]["title"].as_str().map(str::to_string),
        ..Default::default()
    };
    // Entries in document order, whichever shape they come in.
    fn walk(v: &Value, out: &mut Vec<PlaylistItem>, cont: &mut Option<String>) {
        match v {
            Value::Object(m) => {
                for (k, child) in m {
                    match k.as_str() {
                        "lockupViewModel" => out.extend(from_lockup(child)),
                        "playlistVideoRenderer" => out.extend(from_renderer(child)),
                        "continuationCommand" => {
                            if let Some(t) = child["token"].as_str() {
                                *cont = Some(t.to_string());
                            }
                            walk(child, out, cont);
                        }
                        _ => walk(child, out, cont),
                    }
                }
            }
            Value::Array(a) => a.iter().for_each(|c| walk(c, out, cont)),
            _ => {}
        }
    }
    walk(v, &mut page.items, &mut page.continuation);
    Ok(page)
}

async fn browse(http: &reqwest::Client, body: Value) -> Result<Value> {
    let mut payload = json!({ "context": { "client": { "clientName": "WEB", "clientVersion": CLIENT_VERSION, "hl": "en", "gl": "US" } } });
    if let (Some(p), Some(b)) = (payload.as_object_mut(), body.as_object()) {
        p.extend(b.clone());
    }
    let req = http.post(BROWSE).header("Origin", "https://www.youtube.com").json(&payload);
    Ok(send(req, "YouTube").await?.json::<Value>().await?)
}

pub async fn fetch(http: &reqwest::Client, id: &str) -> Result<Fetched> {
    if id.starts_with("RD") {
        bail!("YouTube mixes change every time they're opened, so they can't be imported. Save it as a playlist first.");
    }
    let first = parse_page(&browse(http, json!({ "browseId": format!("VL{id}") })).await?)?;
    let name = first.title.clone().unwrap_or_else(|| "YouTube playlist".into());
    let mut items = first.items;
    let mut cont = first.continuation;
    let mut pages = 1;
    let mut note = None;
    while let Some(token) = cont.take() {
        if pages >= MAX_PAGES {
            note = Some(format!("Only the first {} videos were read.", items.len()));
            break;
        }
        match browse(http, json!({ "continuation": token })).await.and_then(|v| parse_page(&v)) {
            Ok(p) => {
                if p.items.is_empty() {
                    break;
                }
                items.extend(p.items);
                cont = p.continuation;
                pages += 1;
            }
            Err(e) => {
                note = Some(format!("YouTube stopped after {} videos: {e}", items.len()));
                break;
            }
        }
    }
    if items.is_empty() {
        bail!("This YouTube playlist is empty, private, or YouTube's page has changed.");
    }
    Ok(Fetched { playlist: Playlist { name, source: "youtube".into(), source_id: Some(id.to_string()), items }, note })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels() {
        assert_eq!(clean_channel("Kendrick Lamar - Topic"), "Kendrick Lamar");
        assert_eq!(clean_channel("Shakira and 2 more"), "Shakira");
        assert_eq!(clean_channel("Simon and Garfunkel"), "Simon and Garfunkel");
    }

    #[test]
    fn both_entry_shapes_and_continuation() {
        let v: Value = serde_json::from_str(
            r#"{"metadata":{"playlistMetadataRenderer":{"title":"Top 100"}},
            "contents":{"x":[
              {"lockupViewModel":{"contentId":"fcnDmrtj6Sk","contentType":"LOCKUP_CONTENT_TYPE_VIDEO",
                "contentImage":{"thumbnailViewModel":{"overlays":[{"thumbnailBottomOverlayViewModel":{"badges":[{"thumbnailBadgeViewModel":{"text":"4:01"}}]}}]}},
                "metadata":{"lockupMetadataViewModel":{"title":{"content":"Shakira, Burna Boy - Dai Dai (Official Video)"},
                  "metadata":{"contentMetadataViewModel":{"metadataRows":[{"metadataParts":[{"text":{"content":"Shakira and 2 more"}}]},{"metadataParts":[{"text":{"content":"1.2B views"}}]}]}}}}}},
              {"playlistVideoRenderer":{"videoId":"abc","title":{"runs":[{"text":"Swimming Pools (Drank)"}]},
                "shortBylineText":{"runs":[{"text":"Kendrick Lamar - Topic"}]},"lengthSeconds":"313"}},
              {"playlistVideoRenderer":{"videoId":"gone","title":{"runs":[{"text":"[Deleted video]"}]}}},
              {"continuationItemViewModel":{"continuationCommand":{"innertubeCommand":{"continuationCommand":{"token":"NEXT"}}}}}
            ]}}"#,
        )
        .unwrap();
        let p = parse_page(&v).unwrap();
        assert_eq!((p.title.as_deref(), p.items.len(), p.continuation.as_deref()), (Some("Top 100"), 2, Some("NEXT")));
        let a = &p.items[0].track;
        assert_eq!((a.title.as_str(), a.artists.clone(), a.duration_ms), ("Shakira, Burna Boy - Dai Dai (Official Video)", vec!["Shakira".to_string()], Some(241_000)));
        assert_eq!(a.service_ids.get("youtube").map(String::as_str), Some("fcnDmrtj6Sk"));
        let b = &p.items[1].track;
        assert_eq!((b.artists.clone(), b.duration_ms), (vec!["Kendrick Lamar".to_string()], Some(313_000)));
    }
}
