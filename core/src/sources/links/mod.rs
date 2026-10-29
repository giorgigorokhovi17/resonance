//! Paste-a-link playlist import, the way Riff (github.com/rootscripts/riff) does it: public
//! Spotify, YouTube / YouTube Music and Apple Music links are read without signing in.
//!
//! Only track metadata is read (title, artists, album, duration, ids). Tracks are then matched
//! against local files by the playlist matcher; nothing is streamed or downloaded from these
//! services (missing tracks can be found on Soulseek instead, see `sources::missing`).
//!
//! These are the services' own web endpoints, not documented APIs: they can change without
//! notice. Each reader keeps its parsing in pure functions (unit-tested against the shapes seen
//! in October 2026) and fails with a plain message instead of guessing when the shape changes.

pub mod apple;
pub mod spotify;
pub mod youtube;

use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use serde::Serialize;

use crate::library::interchange::Playlist;

/// A desktop browser's user agent; the pages serve their full data to it.
pub const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";

/// What a pasted link points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    SpotifyPlaylist(String),
    SpotifyAlbum(String),
    YouTubePlaylist(String),
    /// The page URL is fetched as given (it carries the storefront and slug).
    ApplePlaylist { url: String, id: String },
    AppleAlbum(String),
}

/// A fetched playlist, plus a note when it's incomplete (a service only gave part of it).
#[derive(Debug, Clone)]
pub struct Fetched {
    pub playlist: Playlist,
    pub note: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LinkInfo {
    pub service: &'static str,
    pub kind: &'static str,
}

impl Link {
    pub fn info(&self) -> LinkInfo {
        match self {
            Link::SpotifyPlaylist(_) => LinkInfo { service: "Spotify", kind: "playlist" },
            Link::SpotifyAlbum(_) => LinkInfo { service: "Spotify", kind: "album" },
            Link::YouTubePlaylist(_) => LinkInfo { service: "YouTube", kind: "playlist" },
            Link::ApplePlaylist { .. } => LinkInfo { service: "Apple Music", kind: "playlist" },
            Link::AppleAlbum(_) => LinkInfo { service: "Apple Music", kind: "album" },
        }
    }
}

fn is_spotify_id(s: &str) -> bool {
    s.len() == 22 && s.chars().all(|c| c.is_ascii_alphanumeric())
}

fn query_param<'a>(url: &'a str, name: &str) -> Option<&'a str> {
    let q = url.split_once('?')?.1;
    let q = q.split('#').next().unwrap_or(q);
    q.split('&').find_map(|kv| kv.strip_prefix(name)?.strip_prefix('='))
}

/// Recognise a pasted link (or Spotify URI). Returns `None` for anything else.
pub fn parse_link(input: &str) -> Option<Link> {
    let s = input.trim();
    // spotify:playlist:<id> / spotify:album:<id>
    if let Some(rest) = s.strip_prefix("spotify:") {
        let (kind, id) = rest.split_once(':')?;
        return match kind {
            "playlist" if is_spotify_id(id) => Some(Link::SpotifyPlaylist(id.into())),
            "album" if is_spotify_id(id) => Some(Link::SpotifyAlbum(id.into())),
            _ => None,
        };
    }
    let no_scheme = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://")).unwrap_or(s);
    let (host, path) = no_scheme.split_once('/').unwrap_or((no_scheme, ""));
    let host = host.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let path_only = path.split(['?', '#']).next().unwrap_or("");
    let segs: Vec<&str> = path_only.split('/').filter(|p| !p.is_empty()).collect();
    match host {
        "open.spotify.com" | "play.spotify.com" => {
            // Optional locale prefix ("intl-de") and "embed".
            let segs: Vec<&str> = segs.into_iter().filter(|p| !p.starts_with("intl-") && *p != "embed").collect();
            match segs.as_slice() {
                ["playlist", id, ..] if is_spotify_id(id) => Some(Link::SpotifyPlaylist(id.to_string())),
                ["album", id, ..] if is_spotify_id(id) => Some(Link::SpotifyAlbum(id.to_string())),
                _ => None,
            }
        }
        "youtube.com" | "m.youtube.com" | "music.youtube.com" | "youtu.be" => {
            let list = query_param(s, "list")?;
            let ok = !list.is_empty() && list.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
            ok.then(|| Link::YouTubePlaylist(list.to_string()))
        }
        "music.apple.com" | "itunes.apple.com" => {
            // /<storefront>/playlist/<slug>/pl.<id>   or   /<storefront>/album/<slug>/<digits>
            if let Some(i) = segs.iter().position(|p| *p == "playlist") {
                let id = segs[i + 1..].iter().find(|p| p.starts_with("pl."))?;
                return Some(Link::ApplePlaylist { url: format!("https://music.apple.com/{}", path_only.trim_start_matches('/')), id: id.to_string() });
            }
            if let Some(i) = segs.iter().position(|p| *p == "album") {
                let id = segs[i + 1..].iter().rev().find(|p| p.chars().all(|c| c.is_ascii_digit()))?;
                return Some(Link::AppleAlbum(id.to_string()));
            }
            None
        }
        _ => None,
    }
}

pub fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder().user_agent(USER_AGENT).timeout(Duration::from_secs(20)).build()?)
}

/// Send a request, waiting out rate limits (429, honouring `Retry-After` up to 10 s) twice.
pub async fn send(req: reqwest::RequestBuilder, what: &str) -> Result<reqwest::Response> {
    for attempt in 0..3 {
        let r = req.try_clone().ok_or_else(|| anyhow!("request can't be retried"))?.send().await;
        let resp = r.map_err(|e| anyhow!("Couldn't reach {what}: {e}"))?;
        if resp.status().as_u16() == 429 && attempt < 2 {
            let wait = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok())
                .unwrap_or(2)
                .min(10);
            tokio::time::sleep(Duration::from_secs(wait.max(1))).await;
            continue;
        }
        return match resp.status().as_u16() {
            200..=299 => Ok(resp),
            404 => bail!("{what} says this link doesn't exist. Is it public?"),
            401 | 403 => bail!("{what} refused the request. Only public playlists can be imported."),
            429 => bail!("{what} is rate-limiting requests. Try again in a minute."),
            code => bail!("{what} answered with an error ({code})."),
        };
    }
    unreachable!("the loop returns on its last attempt")
}

/// Fetch a link's playlist.
pub async fn fetch(link: &Link) -> Result<Fetched> {
    let http = client()?;
    match link {
        Link::SpotifyPlaylist(id) => spotify::fetch(&http, spotify::Kind::Playlist, id).await,
        Link::SpotifyAlbum(id) => spotify::fetch(&http, spotify::Kind::Album, id).await,
        Link::YouTubePlaylist(id) => youtube::fetch(&http, id).await,
        Link::ApplePlaylist { url, id } => apple::fetch_playlist(&http, url, id).await,
        Link::AppleAlbum(id) => apple::fetch_album(&http, id).await,
    }
}

/// Duration text like "4:01" or "1:02:03" in milliseconds.
pub fn clock_ms(text: &str) -> Option<u64> {
    let mut total: u64 = 0;
    let parts: Vec<&str> = text.trim().split(':').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return None;
    }
    for p in parts {
        total = total * 60 + p.trim().parse::<u64>().ok()?;
    }
    Some(total * 1000)
}

/// Walk a JSON tree and collect every object stored under `key`, in document order.
pub fn collect_key<'a>(v: &'a serde_json::Value, key: &str, out: &mut Vec<&'a serde_json::Value>) {
    match v {
        serde_json::Value::Object(m) => {
            for (k, child) in m {
                if k == key {
                    out.push(child);
                }
                collect_key(child, key, out);
            }
        }
        serde_json::Value::Array(a) => a.iter().for_each(|c| collect_key(c, key, out)),
        _ => {}
    }
}

/// Inner text of the first `<script ...>` tag whose opening tag contains `marker`.
pub fn script_with(html: &str, marker: &str) -> Option<String> {
    let mut rest = html;
    while let Some(start) = rest.find("<script") {
        let after = &rest[start..];
        let open_end = after.find('>')?;
        let open = &after[..open_end];
        let body_start = open_end + 1;
        let close = after[body_start..].find("</script>")?;
        if open.contains(marker) {
            return Some(after[body_start..body_start + close].to_string());
        }
        rest = &after[body_start + close..];
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links() {
        let sp = "37i9dQZF1DXcBWIGoYBM5M";
        assert_eq!(parse_link(&format!("https://open.spotify.com/playlist/{sp}?si=abc")), Some(Link::SpotifyPlaylist(sp.into())));
        assert_eq!(parse_link(&format!("open.spotify.com/intl-de/album/{sp}")), Some(Link::SpotifyAlbum(sp.into())));
        assert_eq!(parse_link(&format!("https://open.spotify.com/embed/playlist/{sp}")), Some(Link::SpotifyPlaylist(sp.into())));
        assert_eq!(parse_link(&format!("spotify:playlist:{sp}")), Some(Link::SpotifyPlaylist(sp.into())));
        assert_eq!(parse_link("https://open.spotify.com/track/37i9dQZF1DXcBWIGoYBM5M"), None, "single tracks aren't playlists");
        assert_eq!(
            parse_link("https://music.youtube.com/playlist?list=PL4fGSI1pDJn6puJdseH2Rt9sMvt9E2M4i&si=x"),
            Some(Link::YouTubePlaylist("PL4fGSI1pDJn6puJdseH2Rt9sMvt9E2M4i".into()))
        );
        assert_eq!(parse_link("https://www.youtube.com/watch?v=abc&list=OLAK5uy_k"), Some(Link::YouTubePlaylist("OLAK5uy_k".into())));
        assert_eq!(parse_link("https://www.youtube.com/watch?v=abc"), None);
        assert_eq!(
            parse_link("https://music.apple.com/us/playlist/todays-hits/pl.f4d106fed2bd41149aaacabb233eb5eb"),
            Some(Link::ApplePlaylist {
                url: "https://music.apple.com/us/playlist/todays-hits/pl.f4d106fed2bd41149aaacabb233eb5eb".into(),
                id: "pl.f4d106fed2bd41149aaacabb233eb5eb".into()
            })
        );
        assert_eq!(parse_link("https://music.apple.com/us/album/expectations/6822390423?i=6822390426"), Some(Link::AppleAlbum("6822390423".into())));
        assert_eq!(parse_link("https://example.com/playlist/x"), None);
        assert_eq!(parse_link(""), None);
    }

    #[test]
    fn clock() {
        assert_eq!(clock_ms("4:01"), Some(241_000));
        assert_eq!(clock_ms("1:02:03"), Some(3_723_000));
        assert_eq!(clock_ms("live"), None);
        assert_eq!(clock_ms("42"), None);
    }

    #[test]
    fn scripts() {
        let h = r#"<html><script src="a.js"></script><script id="x" type="application/json">{"a":1}</script></html>"#;
        assert_eq!(script_with(h, r#"id="x""#).as_deref(), Some(r#"{"a":1}"#));
        assert_eq!(script_with(h, "nope"), None);
    }
}
