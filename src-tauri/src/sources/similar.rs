//! Songs like a playlist's that the user doesn't have, from Deezer's public API
//! (https://developers.deezer.com/api, no account or key). Only runs when the user clicks
//! "Find more online" on a playlist; sends the names of the playlist's main artists, nothing else.
//! Only names come back: Deezer's audio previews are never fetched or played (see AGENTS.md).

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::library::matcher::normalize::{key, parse_title, split_artists};
use crate::library::model::Track;
use crate::library::recommend::credited;

const API: &str = "https://api.deezer.com";
const USER_AGENT: &str = concat!("Undertone/", env!("CARGO_PKG_VERSION"), " (https://github.com/giorgigorokhovi17/resonance)");
/// Playlist artists to look up, most frequent first.
const SEEDS: usize = 5;
/// Similar artists whose top songs are fetched.
const RELATED: usize = 8;
/// Songs suggested per artist.
const PER_ARTIST: usize = 2;
/// Deezer allows 50 requests per 5 seconds; this keeps well under it.
const BETWEEN_REQUESTS: Duration = Duration::from_millis(120);

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(default)]
struct DzArtist {
    id: u64,
    name: String,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(default)]
struct DzAlbum {
    title: String,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(default)]
struct DzTrack {
    id: u64,
    title: String,
    title_short: String,
    /// Seconds.
    duration: u64,
    artist: DzArtist,
    album: DzAlbum,
    contributors: Vec<DzArtist>,
}

#[derive(Deserialize, Debug)]
struct DzError {
    code: Option<i64>,
    message: Option<String>,
}

#[derive(Deserialize, Debug)]
struct DzList<T> {
    #[serde(default = "Vec::new")]
    data: Vec<T>,
    error: Option<DzError>,
}

/// A song the user doesn't have, as Deezer names it.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OnlineSuggestion {
    pub deezer_id: u64,
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    /// Seconds; 0 when unknown.
    pub duration: f64,
    /// Why, in a few words ("Similar to Daft Punk").
    pub reason: String,
}

/// The playlist's most credited artists (display name, how many songs), most first.
pub fn seed_artists(members: &[Track], max: usize) -> Vec<(String, usize)> {
    let mut count: HashMap<String, (String, usize)> = HashMap::new();
    for t in members {
        // The main credit only: featured names are looked up when they lead a song themselves.
        let main = split_artists(&t.artist).into_iter().next().unwrap_or_default();
        let shown = first_credit(&t.artist);
        if main.is_empty() || main == "variousartists" {
            continue;
        }
        count.entry(main).or_insert((shown, 0)).1 += 1;
    }
    let mut v: Vec<(String, usize)> = count.into_values().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.truncate(max);
    v
}

/// "Daft Punk feat. Pharrell Williams" → "Daft Punk".
fn first_credit(artist: &str) -> String {
    let lower = artist.to_ascii_lowercase();
    let mut end = artist.len();
    for sep in [";", " / ", ",", " & ", " feat.", " feat ", " ft.", " featuring ", " x ", " with "] {
        if let Some(i) = lower.find(sep) {
            end = end.min(i);
        }
    }
    artist[..end].trim().to_string()
}

/// What the user already has, by (title, artist) keys, so owned songs aren't suggested.
pub struct Owned(HashSet<(String, String)>);

impl Owned {
    pub fn new(library: &[Track]) -> Self {
        let mut set = HashSet::new();
        for t in library {
            let title = key(&parse_title(&t.title).main);
            for a in credited(t) {
                set.insert((title.clone(), a));
            }
        }
        Self(set)
    }

    fn has(&self, title: &str, artists: &[String]) -> bool {
        let title = key(&parse_title(title).main);
        artists.iter().flat_map(|a| split_artists(a)).any(|a| self.0.contains(&(title.clone(), a)))
    }
}

/// Similar artists across all seeds, best first: an artist similar to several of the playlist's
/// artists, or high on a frequent artist's list, wins. Seeds themselves are left out.
fn rank_related(related: &[((String, usize), Vec<DzArtist>)], max: usize) -> Vec<(DzArtist, String)> {
    let seeds: HashSet<String> = related.iter().map(|((s, _), _)| key(s)).collect();
    let mut score: HashMap<u64, (f64, DzArtist, String)> = HashMap::new();
    for ((seed, weight), list) in related {
        let len = list.len().max(1) as f64;
        for (rank, a) in list.iter().enumerate() {
            if seeds.contains(&key(&a.name)) {
                continue;
            }
            let s = *weight as f64 * (1.0 - rank as f64 / (len + 1.0));
            let e = score.entry(a.id).or_insert((0.0, a.clone(), seed.clone()));
            e.0 += s;
        }
    }
    let mut v: Vec<(f64, DzArtist, String)> = score.into_values().collect();
    v.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
    v.into_iter().take(max).map(|(_, a, seed)| (a, seed)).collect()
}

/// Up to `n` of an artist's top songs that the user doesn't have and that aren't already picked.
fn pick_tracks(top: &[DzTrack], owned: &Owned, picked: &mut HashSet<String>, n: usize, reason: &str) -> Vec<OnlineSuggestion> {
    let mut out = Vec::new();
    for t in top {
        if out.len() >= n {
            break;
        }
        let title = if t.title.trim().is_empty() { t.title_short.trim() } else { t.title.trim() };
        let mut artists: Vec<String> = t.contributors.iter().map(|c| c.name.trim().to_string()).filter(|c| !c.is_empty()).collect();
        if artists.is_empty() && !t.artist.name.trim().is_empty() {
            artists.push(t.artist.name.trim().to_string());
        }
        if title.is_empty() || artists.is_empty() || owned.has(title, &artists) {
            continue;
        }
        let k = format!("{}\x1f{}", key(&parse_title(title).main), key(&artists[0]));
        if !picked.insert(k) {
            continue;
        }
        out.push(OnlineSuggestion {
            deezer_id: t.id,
            title: title.to_string(),
            artists,
            album: t.album.title.trim().to_string(),
            duration: t.duration as f64,
            reason: reason.to_string(),
        });
    }
    out
}

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder().user_agent(USER_AGENT).timeout(Duration::from_secs(15)).build()?)
}

fn parse_list<T: serde::de::DeserializeOwned>(body: &str) -> Result<Vec<T>> {
    let list: DzList<T> = serde_json::from_str(body).map_err(|_| anyhow!("Deezer sent something unexpected."))?;
    match list.error {
        Some(DzError { code: Some(4), .. }) => Err(anyhow!("Deezer is busy. Try again in a minute.")),
        Some(e) => Err(anyhow!("Deezer answered with an error: {}", e.message.unwrap_or_default())),
        None => Ok(list.data),
    }
}

async fn get_list<T: serde::de::DeserializeOwned>(http: &reqwest::Client, path: &str) -> Result<Vec<T>> {
    tokio::time::sleep(BETWEEN_REQUESTS).await;
    let resp = http
        .get(format!("{API}{path}"))
        .send()
        .await
        .map_err(|_| anyhow!("Couldn't reach Deezer. Check your internet connection."))?;
    match resp.status().as_u16() {
        200..=299 => parse_list(&resp.text().await.map_err(|_| anyhow!("Deezer sent something unexpected."))?),
        429 => Err(anyhow!("Deezer is busy. Try again in a minute.")),
        code => Err(anyhow!("Deezer answered with an error ({code}).")),
    }
}

fn enc(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
}

/// Songs like the playlist's that aren't in the library: a couple of top songs from each of
/// the playlist's artists the user doesn't have yet, then from artists Deezer lists as similar.
pub async fn find(members: &[Track], library: &[Track], limit: usize) -> Result<Vec<OnlineSuggestion>> {
    let seeds = seed_artists(members, SEEDS);
    if seeds.is_empty() {
        return Ok(Vec::new());
    }
    let http = client()?;
    let owned = Owned::new(library);
    let mut picked: HashSet<String> = HashSet::new();
    let mut found: Vec<(DzArtist, (String, usize))> = Vec::new();
    for (name, weight) in &seeds {
        let hits: Vec<DzArtist> = get_list(&http, &format!("/search/artist?q={}&limit=3", enc(name))).await?;
        // Only the same artist: a wrong one would make every suggestion wrong.
        if let Some(a) = hits.into_iter().find(|a| key(&a.name) == key(name)) {
            found.push((a, (name.clone(), *weight)));
        }
    }
    let mut related: Vec<((String, usize), Vec<DzArtist>)> = Vec::new();
    for (a, seed) in &found {
        related.push((seed.clone(), get_list(&http, &format!("/artist/{}/related?limit=10", a.id)).await?));
    }
    let mut out = Vec::new();
    // More from the playlist's own top artists first (just the two most frequent).
    for (a, (name, _)) in found.iter().take(2) {
        let top: Vec<DzTrack> = get_list(&http, &format!("/artist/{}/top?limit=10", a.id)).await?;
        out.extend(pick_tracks(&top, &owned, &mut picked, PER_ARTIST, &format!("More from {name}")));
    }
    for (a, seed) in rank_related(&related, RELATED) {
        if out.len() >= limit {
            break;
        }
        let top: Vec<DzTrack> = get_list(&http, &format!("/artist/{}/top?limit=5", a.id)).await?;
        out.extend(pick_tracks(&top, &owned, &mut picked, PER_ARTIST, &format!("Similar to {seed}")));
    }
    out.truncate(limit);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(title: &str, artist: &str) -> Track {
        Track { path: format!("{artist} - {title}"), title: title.into(), artist: artist.into(), ..Default::default() }
    }

    fn dz(id: u64, title: &str, artists: &[&str]) -> DzTrack {
        DzTrack {
            id,
            title: title.into(),
            artist: DzArtist { id: 1, name: artists[0].into() },
            contributors: artists.iter().map(|a| DzArtist { id: 1, name: (*a).into() }).collect(),
            duration: 200,
            ..Default::default()
        }
    }

    fn artist(id: u64, name: &str) -> DzArtist {
        DzArtist { id, name: name.into() }
    }

    #[test]
    fn seeds_are_main_artists_by_count() {
        let members = vec![
            t("Get Lucky", "Daft Punk feat. Pharrell Williams"),
            t("One More Time", "Daft Punk"),
            t("Happy", "Pharrell Williams"),
            t("Compilation Track", "Various Artists"),
        ];
        assert_eq!(seed_artists(&members, 5), vec![("Daft Punk".to_string(), 2), ("Pharrell Williams".to_string(), 1)]);
        assert_eq!(seed_artists(&members, 1).len(), 1);
    }

    #[test]
    fn owned_songs_and_repeats_are_not_suggested() {
        let owned = Owned::new(&[t("Instant Crush", "Daft Punk")]);
        let top = vec![
            dz(1, "Instant Crush (feat. Julian Casablancas)", &["Daft Punk", "Julian Casablancas"]),
            dz(2, "Get Lucky", &["Daft Punk", "Pharrell Williams"]),
            dz(3, "Get Lucky (Radio Edit)", &["Daft Punk"]),
            dz(4, "Lose Yourself to Dance", &["Daft Punk"]),
            dz(5, "Harder, Better, Faster, Stronger", &["Daft Punk"]),
        ];
        let mut picked = HashSet::new();
        let got = pick_tracks(&top, &owned, &mut picked, 2, "More from Daft Punk");
        let ids: Vec<u64> = got.iter().map(|s| s.deezer_id).collect();
        assert_eq!(ids, vec![2, 4], "owned Instant Crush and a second Get Lucky are skipped");
        assert_eq!(got[0].artists, vec!["Daft Punk".to_string(), "Pharrell Williams".to_string()]);
        // Already picked for another artist: not again.
        assert!(pick_tracks(&top[1..2], &owned, &mut picked, 2, "").is_empty());
    }

    #[test]
    fn related_artists_shared_by_seeds_rank_first() {
        let related = vec![
            (("Daft Punk".to_string(), 3), vec![artist(10, "Justice"), artist(11, "Cassius"), artist(12, "Pharrell Williams")]),
            (("Pharrell Williams".to_string(), 1), vec![artist(20, "N.E.R.D"), artist(11, "Cassius")]),
        ];
        let ranked = rank_related(&related, 10);
        let names: Vec<&str> = ranked.iter().map(|(a, _)| a.name.as_str()).collect();
        assert!(!names.contains(&"Pharrell Williams"), "a seed isn't its own suggestion");
        assert_eq!(names[0], "Justice");
        assert!(names.iter().position(|n| *n == "Cassius") < names.iter().position(|n| *n == "N.E.R.D"));
        assert_eq!(ranked[0].1, "Daft Punk");
        assert_eq!(rank_related(&related, 1).len(), 1);
    }

    #[test]
    fn deezer_errors_are_plain() {
        let busy = parse_list::<DzArtist>(r#"{"error":{"type":"Exception","message":"Quota limit exceeded","code":4}}"#);
        assert_eq!(busy.unwrap_err().to_string(), "Deezer is busy. Try again in a minute.");
        let ok = parse_list::<DzArtist>(r#"{"data":[{"id":27,"name":"Daft Punk","link":"x"}],"total":1}"#).unwrap();
        assert_eq!(ok[0].id, 27);
        assert!(parse_list::<DzArtist>("<html>").is_err());
    }
}
