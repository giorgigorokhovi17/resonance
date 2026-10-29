//! Lyrics search on LRCLIB (https://lrclib.net), a free, open lyrics database with no account or
//! key. Only runs when the user asks for a song's lyrics; sends the song's title, artist, album and
//! length. Wrong lyrics are worse than none: a result must have the same length (within 3 s) when
//! both lengths are known.

use std::time::Duration;

use anyhow::{anyhow, Result};
use serde::Deserialize;

use crate::library::model::Track;

const API: &str = "https://lrclib.net/api";
/// LRCLIB asks apps to name themselves.
const USER_AGENT: &str = concat!("Undertone/", env!("CARGO_PKG_VERSION"), " (https://github.com/giorgigorokhovi17/resonance)");
/// Largest length difference between the song and a search result, in seconds.
const MAX_DRIFT: f64 = 3.0;

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase", default)]
struct Record {
    track_name: Option<String>,
    artist_name: Option<String>,
    duration: Option<f64>,
    instrumental: bool,
    plain_lyrics: Option<String>,
    synced_lyrics: Option<String>,
}

#[derive(Debug, PartialEq)]
pub enum Found {
    /// LRC or plain text, timed lyrics preferred.
    Lyrics(String),
    Instrumental,
    Nothing,
}

fn text(r: &Record) -> Option<String> {
    [&r.synced_lyrics, &r.plain_lyrics]
        .into_iter()
        .flatten()
        .find(|t| !t.trim().is_empty())
        .cloned()
}

fn found(r: &Record) -> Found {
    match text(r) {
        Some(t) => Found::Lyrics(t),
        None if r.instrumental => Found::Instrumental,
        None => Found::Nothing,
    }
}

/// "Song (Remastered 2011)" → "Song", "Song - Live" → "Song".
fn bare_title(title: &str) -> String {
    let mut t = title.trim().to_string();
    for (open, close) in [('(', ')'), ('[', ']')] {
        while let (Some(i), true) = (t.rfind(open), t.ends_with(close)) {
            t = t[..i].trim_end().to_string();
        }
    }
    if let Some(i) = t.find(" - ") {
        t.truncate(i);
    }
    let t = t.trim();
    if t.is_empty() { title.trim().to_string() } else { t.to_string() }
}

/// The first of several credited artists ("A feat. B", "A; B", "A, B", "A & B").
fn main_artist(artist: &str) -> String {
    let lower = artist.to_lowercase();
    let mut end = artist.len();
    for sep in [";", " / ", ",", " & ", " feat.", " feat ", " ft.", " featuring ", " x "] {
        if let Some(i) = lower.find(sep) {
            end = end.min(i);
        }
    }
    artist[..end].trim().to_string()
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// The best search result for the song: same length, same title once trimmed, and an artist
/// that overlaps; timed lyrics win over plain ones.
fn pick<'a>(results: &'a [Record], title: &str, artist: &str, duration: f64) -> Option<&'a Record> {
    let want_title = norm(&bare_title(title));
    let want_artist = norm(&main_artist(artist));
    results
        .iter()
        .filter(|r| text(r).is_some() || r.instrumental)
        .filter(|r| match r.duration {
            Some(d) if d > 0.0 && duration > 0.0 => (d - duration).abs() <= MAX_DRIFT,
            _ => duration <= 0.0,
        })
        .filter(|r| norm(&bare_title(r.track_name.as_deref().unwrap_or_default())) == want_title)
        .filter(|r| {
            let a = norm(r.artist_name.as_deref().unwrap_or_default());
            want_artist.is_empty() || a.contains(&want_artist) || (!a.is_empty() && want_artist.contains(&a))
        })
        .max_by_key(|r| (r.synced_lyrics.as_deref().is_some_and(|t| !t.trim().is_empty()), !r.instrumental))
}

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder().user_agent(USER_AGENT).timeout(Duration::from_secs(15)).build()?)
}

async fn get_json<T: serde::de::DeserializeOwned>(http: &reqwest::Client, url: &str, query: &[(&str, String)]) -> Result<Option<T>> {
    let qs: Vec<String> = query
        .iter()
        .map(|(k, v)| format!("{k}={}", percent_encoding::utf8_percent_encode(v, percent_encoding::NON_ALPHANUMERIC)))
        .collect();
    let resp = http
        .get(format!("{url}?{}", qs.join("&")))
        .send()
        .await
        .map_err(|_| anyhow!("Couldn't reach LRCLIB. Check your internet connection."))?;
    match resp.status().as_u16() {
        404 => Ok(None),
        200..=299 => Ok(Some(resp.json::<T>().await.map_err(|_| anyhow!("LRCLIB sent something unexpected."))?)),
        429 => Err(anyhow!("LRCLIB is busy. Try again in a minute.")),
        code => Err(anyhow!("LRCLIB answered with an error ({code}).")),
    }
}

/// Looks the song up: first by its exact title, artist, album and length, then by a search.
pub async fn find(track: &Track) -> Result<Found> {
    let http = client()?;
    let artist = if track.artist.trim().is_empty() { &track.album_artist } else { &track.artist };
    let title = track.title.trim();
    if title.is_empty() {
        return Ok(Found::Nothing);
    }
    let mut exact = vec![("track_name", title.to_string()), ("artist_name", artist.trim().to_string())];
    if !track.album.trim().is_empty() {
        exact.push(("album_name", track.album.trim().to_string()));
    }
    if track.duration > 0.0 {
        exact.push(("duration", format!("{}", track.duration.round() as i64)));
    }
    if !artist.trim().is_empty() {
        if let Some(r) = get_json::<Record>(&http, &format!("{API}/get"), &exact).await? {
            let f = found(&r);
            if f != Found::Nothing {
                return Ok(f);
            }
        }
    }
    let mut query = vec![("track_name", bare_title(title))];
    let main = main_artist(artist);
    if !main.is_empty() {
        query.push(("artist_name", main));
    }
    let results = get_json::<Vec<Record>>(&http, &format!("{API}/search"), &query).await?.unwrap_or_default();
    Ok(pick(&results, title, artist, track.duration).map(found).unwrap_or(Found::Nothing))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(title: &str, artist: &str, duration: f64, synced: Option<&str>, plain: Option<&str>) -> Record {
        Record {
            track_name: Some(title.into()),
            artist_name: Some(artist.into()),
            duration: Some(duration),
            instrumental: false,
            plain_lyrics: plain.map(str::to_string),
            synced_lyrics: synced.map(str::to_string),
        }
    }

    #[test]
    fn titles_and_artists_are_trimmed_for_search() {
        assert_eq!(bare_title("Song (Remastered 2011)"), "Song");
        assert_eq!(bare_title("Song [Live] (Mono)"), "Song");
        assert_eq!(bare_title("Song - 2009 Remaster"), "Song");
        assert_eq!(bare_title("(Untitled)"), "(Untitled)");
        assert_eq!(main_artist("Daft Punk feat. Pharrell Williams"), "Daft Punk");
        assert_eq!(main_artist("A; B"), "A");
        assert_eq!(main_artist("Simon & Garfunkel"), "Simon");
    }

    #[test]
    fn picks_same_length_and_prefers_timed_lyrics() {
        let results = vec![
            rec("Creep", "Radiohead", 238.0, None, Some("plain")),
            rec("Creep", "Radiohead", 239.5, Some("[00:01.00]timed"), None),
            rec("Creep", "Radiohead", 260.0, Some("[00:01.00]other version"), None),
            rec("Creep", "TLC", 238.0, Some("[00:01.00]wrong song"), None),
        ];
        let r = pick(&results, "Creep", "Radiohead", 238.4).unwrap();
        assert_eq!(found(r), Found::Lyrics("[00:01.00]timed".into()));
    }

    #[test]
    fn no_guessing_when_nothing_fits() {
        let results = vec![
            rec("Creep", "Radiohead", 260.0, Some("[00:01.00]x"), None),
            rec("Creep (Acoustic)", "Radiohead", 238.0, Some("[00:01.00]x"), None),
            rec("Creeping", "Radiohead", 238.0, Some("[00:01.00]x"), None),
            rec("Creep", "Radiohead", 238.0, None, Some("  ")),
        ];
        let r = pick(&results, "Creep", "Radiohead", 300.0);
        assert!(r.is_none());
        assert!(pick(&results[2..], "Creep", "Radiohead", 238.0).is_none());
    }

    #[test]
    fn instrumentals_say_so() {
        let mut r = rec("Intro", "X", 60.0, None, None);
        r.instrumental = true;
        assert_eq!(found(&r), Found::Instrumental);
        let parsed: Vec<Record> = serde_json::from_str(
            r#"[{"id":1,"trackName":"Intro","artistName":"X","albumName":"Y","duration":60.0,"instrumental":false,"plainLyrics":"a","syncedLyrics":null}]"#,
        )
        .unwrap();
        assert_eq!(found(&parsed[0]), Found::Lyrics("a".into()));
    }
}
