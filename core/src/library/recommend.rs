//! "Recommended songs" for a playlist, from the user's own library: songs that aren't in it yet,
//! scored by what they share with the songs that are (artists, featured artists included, album,
//! genre, era) and by listening history (songs often played right before or after the playlist's
//! songs). Nothing leaves the PC.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use rusqlite::Connection;
use serde::Serialize;

use super::matcher::normalize::{key, parse_title, split_artists};
use super::model::Track;

/// Two plays started this close together count as played next to each other.
const NEIGHBOUR_GAP: i64 = 20 * 60;
/// Below this a song shares too little with the playlist to suggest.
const MIN_SCORE: f64 = 1.0;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Recommendation {
    pub track: Track,
    pub score: f64,
    /// Why, in a few words ("More from Daft Punk").
    pub reason: String,
}

/// Everyone credited on a song: the artist and album artist tags, and "feat." names in the title.
pub fn credited(t: &Track) -> Vec<String> {
    let mut out = split_artists(&t.artist);
    for a in split_artists(&t.album_artist).into_iter().chain(parse_title(&t.title).featured.iter().map(|a| key(a))) {
        if !a.is_empty() && !out.contains(&a) && a != "variousartists" {
            out.push(a);
        }
    }
    out.retain(|a| a != "variousartists");
    out
}

fn genres(t: &Track) -> Vec<String> {
    t.genre.split([';', ',', '/']).map(key).filter(|g| !g.is_empty()).collect()
}

fn title_key(t: &Track) -> String {
    key(&parse_title(&t.title).main)
}

/// The name to show for a credited artist key, as one of the songs' tags spells it.
fn shown_artist(key_: &str, tracks: &[&Track]) -> String {
    const SEPS: [&str; 13] = [" feat. ", " feat ", " ft. ", " featuring ", " & ", " and ", " x ", " with ", " vs ", ",", ";", " / ", "/"];
    for t in tracks {
        for tag in [&t.artist, &t.album_artist] {
            // ASCII lowercasing keeps byte positions, so the cuts line up with the original.
            let lower = tag.to_ascii_lowercase();
            let mut cuts = vec![(0, 0)];
            for sep in SEPS {
                cuts.extend(lower.match_indices(sep).map(|(i, m)| (i, i + m.len())));
            }
            cuts.sort_unstable();
            for (k, &(_, start)) in cuts.iter().enumerate() {
                let end = cuts.get(k + 1).map(|c| c.0).unwrap_or(tag.len());
                if let Some(part) = tag.get(start..end.max(start)) {
                    if key(part) == key_ {
                        return part.trim().to_string();
                    }
                }
            }
        }
    }
    key_.to_string()
}

/// How often each pair of songs was played one right after the other (both orders), from plays
/// as (path, started at) in any order.
pub fn neighbours(plays: &[(String, i64)]) -> HashMap<(String, String), u32> {
    let mut sorted: Vec<&(String, i64)> = plays.iter().collect();
    sorted.sort_by_key(|p| p.1);
    let mut out: HashMap<(String, String), u32> = HashMap::new();
    for w in sorted.windows(2) {
        let (a, b) = (w[0], w[1]);
        if a.0 != b.0 && b.1 - a.1 <= NEIGHBOUR_GAP {
            *out.entry((a.0.clone(), b.0.clone())).or_default() += 1;
            *out.entry((b.0.clone(), a.0.clone())).or_default() += 1;
        }
    }
    out
}

/// Library songs that fit a playlist, best first. `playlist` holds the paths in it; `plays` is
/// the listening history as (path, started at).
pub fn recommend(library: &[Track], playlist: &[String], plays: &[(String, i64)], limit: usize) -> Vec<Recommendation> {
    let in_list: HashSet<&str> = playlist.iter().map(String::as_str).collect();
    let members: Vec<&Track> = library.iter().filter(|t| in_list.contains(t.path.as_str())).collect();
    if members.is_empty() {
        return Vec::new();
    }
    let n = members.len() as f64;
    let mut artist_count: HashMap<String, usize> = HashMap::new();
    let mut album_count: HashMap<String, usize> = HashMap::new();
    let mut genre_count: HashMap<String, usize> = HashMap::new();
    let mut songs: HashSet<(String, String)> = HashSet::new();
    let mut years: Vec<i32> = Vec::new();
    for t in &members {
        let artists = credited(t);
        for a in &artists {
            *artist_count.entry(a.clone()).or_default() += 1;
            songs.insert((title_key(t), a.clone()));
        }
        let album = key(&t.album);
        if !album.is_empty() {
            *album_count.entry(album).or_default() += 1;
        }
        let mut seen = HashSet::new();
        for g in genres(t) {
            if seen.insert(g.clone()) {
                *genre_count.entry(g).or_default() += 1;
            }
        }
        years.extend(t.year.filter(|y| *y > 0));
    }
    years.sort_unstable();
    let mid_year = years.get(years.len() / 2).copied();

    let next_to = neighbours(plays);
    let mut play_count: HashMap<&str, u32> = HashMap::new();
    for (p, _) in plays {
        *play_count.entry(p.as_str()).or_default() += 1;
    }

    let mut out: Vec<Recommendation> = Vec::new();
    for c in library.iter().filter(|t| !in_list.contains(t.path.as_str())) {
        let artists = credited(c);
        let tk = title_key(c);
        // Another copy or version of a song that's already in (or already suggested).
        if !tk.is_empty() && artists.iter().any(|a| songs.contains(&(tk.clone(), a.clone()))) {
            continue;
        }
        // Each signal scores 0..its weight; the reason is the strongest one.
        let mut signals: Vec<(f64, String)> = Vec::new();
        if let Some((a, k)) = artists.iter().filter_map(|a| artist_count.get(a).map(|k| (a, *k))).max_by_key(|(_, k)| *k) {
            let name = shown_artist(a, &[&[c], members.as_slice()].concat());
            signals.push((1.5 + 3.0 * k as f64 / n, format!("More from {name}")));
        }
        let album = key(&c.album);
        if let Some(k) = album_count.get(&album).filter(|_| !album.is_empty()) {
            signals.push((1.0 + 1.0 * *k as f64 / n, format!("From {}", c.album.trim())));
        }
        if let Some((_, k)) = genres(c).into_iter().filter_map(|g| genre_count.get(&g).map(|k| (g, *k))).max_by_key(|(_, k)| *k) {
            let shown = c.genre.split([';', ',', '/']).next().unwrap_or("").trim().to_string();
            signals.push((2.0 * k as f64 / n, format!("Also {shown}")));
        }
        let mut best_neighbour: Option<(&Track, u32)> = None;
        let mut together = 0u32;
        for m in &members {
            if let Some(k) = next_to.get(&(c.path.clone(), m.path.clone())) {
                together += k;
                if best_neighbour.is_none_or(|(_, b)| *k > b) {
                    best_neighbour = Some((m, *k));
                }
            }
        }
        if let Some((m, _)) = best_neighbour {
            signals.push(((together as f64).min(3.0), format!("Often played with {}", m.title.trim())));
        }
        if signals.is_empty() {
            continue;
        }
        if let (Some(mid), Some(y)) = (mid_year, c.year.filter(|y| *y > 0)) {
            if (y - mid).abs() <= 3 {
                signals.push((0.5, format!("Also from around {mid}")));
            }
        }
        let mut score: f64 = signals.iter().map(|s| s.0).sum();
        if score < MIN_SCORE {
            continue;
        }
        // Songs the user plays a lot win ties.
        score += 0.25 * (1.0 + *play_count.get(c.path.as_str()).unwrap_or(&0) as f64).ln();
        let reason = signals.into_iter().max_by(|a, b| a.0.total_cmp(&b.0)).map(|s| s.1).unwrap_or_default();
        out.push(Recommendation { track: c.clone(), score: (score * 100.0).round() / 100.0, reason });
    }
    out.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.track.title.cmp(&b.track.title)));
    // Two copies of one song: keep the better-scored one.
    let mut seen_songs: HashSet<(String, String)> = HashSet::new();
    out.retain(|r| {
        let tk = title_key(&r.track);
        tk.is_empty() || credited(&r.track).first().is_none_or(|a| seen_songs.insert((tk, a.clone())))
    });
    out.truncate(limit);
    out
}

/// Every play as (path, started at), for [`recommend`].
pub fn play_history(conn: &Connection) -> Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare("SELECT path, played_at FROM plays ORDER BY played_at")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(path: &str, title: &str, artist: &str, album: &str, genre: &str, year: Option<i32>) -> Track {
        Track {
            path: path.into(),
            title: title.into(),
            artist: artist.into(),
            album: album.into(),
            genre: genre.into(),
            year,
            ..Default::default()
        }
    }

    fn paths(r: &[Recommendation]) -> Vec<&str> {
        r.iter().map(|r| r.track.path.as_str()).collect()
    }

    fn library() -> Vec<Track> {
        vec![
            t("a1", "One More Time", "Daft Punk", "Discovery", "Electronic", Some(2001)),
            t("a2", "Digital Love", "Daft Punk", "Discovery", "Electronic", Some(2001)),
            t("a3", "Around the World", "Daft Punk", "Homework", "Electronic", Some(1997)),
            t("b1", "Get Lucky", "Daft Punk feat. Pharrell Williams", "Random Access Memories", "Disco", Some(2013)),
            t("c1", "Happy", "Pharrell Williams", "G I R L", "Pop", Some(2014)),
            t("d1", "Music Sounds Better With You", "Stardust", "", "Electronic", Some(1998)),
            t("e1", "Jolene", "Dolly Parton", "Jolene", "Country", Some(1974)),
        ]
    }

    #[test]
    fn same_artist_comes_first_and_nothing_in_the_playlist_is_suggested() {
        let lib = library();
        let r = recommend(&lib, &["a1".into(), "a2".into()], &[], 10);
        assert!(!paths(&r).contains(&"a1") && !paths(&r).contains(&"a2"));
        assert_eq!(r[0].track.path, "a3");
        assert_eq!(r[0].reason, "More from Daft Punk");
        assert!(paths(&r).contains(&"b1"), "a featured credit still counts as the artist");
        assert!(!paths(&r).contains(&"e1"), "nothing in common: not suggested");
    }

    #[test]
    fn featured_artists_link_songs() {
        let lib = library();
        let r = recommend(&lib, &["c1".into()], &[], 10);
        assert_eq!(paths(&r).first(), Some(&"b1"), "Pharrell is featured on Get Lucky");
        assert_eq!(r[0].reason, "More from Pharrell Williams");
    }

    #[test]
    fn genre_alone_is_enough_when_it_is_the_whole_playlist() {
        let lib = library();
        let r = recommend(&lib, &["a1".into()], &[], 10);
        assert!(paths(&r).contains(&"d1"));
        let d1 = r.iter().find(|r| r.track.path == "d1").unwrap();
        assert_eq!(d1.reason, "Also Electronic");
        assert!(r.iter().position(|r| r.track.path == "a3") < r.iter().position(|r| r.track.path == "d1"));
    }

    #[test]
    fn songs_played_next_to_the_playlist_are_suggested() {
        let lib = library();
        // Jolene shares nothing with Happy, but they're always played back to back.
        let plays = vec![
            ("c1".to_string(), 1000),
            ("e1".to_string(), 1240),
            ("c1".to_string(), 5000),
            ("e1".to_string(), 5230),
            // Hours apart: not next to each other.
            ("e1".to_string(), 50_000),
        ];
        let r = recommend(&lib, &["c1".into()], &plays, 10);
        let e1 = r.iter().find(|r| r.track.path == "e1").expect("suggested from history");
        assert_eq!(e1.reason, "Often played with Happy");
    }

    #[test]
    fn neighbours_need_plays_close_together() {
        let n = neighbours(&[("x".into(), 0), ("y".into(), 300), ("z".into(), 300 + NEIGHBOUR_GAP + 1), ("z".into(), 400 + NEIGHBOUR_GAP)]);
        assert_eq!(n.get(&("x".into(), "y".into())), Some(&1));
        assert_eq!(n.get(&("y".into(), "x".into())), Some(&1));
        assert_eq!(n.get(&("y".into(), "z".into())), None);
        assert_eq!(n.get(&("z".into(), "z".into())), None, "a repeat isn't a neighbour");
    }

    #[test]
    fn other_copies_of_songs_already_in_are_left_out() {
        let mut lib = library();
        lib.push(t("a1b", "One More Time (Radio Edit)", "Daft Punk", "Singles", "Electronic", Some(2000)));
        lib.push(t("a3b", "Around The World", "Daft Punk", "Musique", "Electronic", Some(2006)));
        let r = recommend(&lib, &["a1".into()], &[], 10);
        assert!(!paths(&r).contains(&"a1b"));
        // Two copies of the same suggestion: only one is shown.
        assert_eq!(paths(&r).iter().filter(|p| p.starts_with("a3")).count(), 1);
    }

    #[test]
    fn play_count_breaks_ties_and_limit_applies() {
        let lib = library();
        let plays: Vec<(String, i64)> = (0..5).map(|i| ("a3".to_string(), i * 100_000)).collect();
        let base = recommend(&lib, &["b1".into()], &[], 10);
        let played = recommend(&lib, &["b1".into()], &plays, 10);
        let score = |r: &[Recommendation]| r.iter().find(|r| r.track.path == "a3").unwrap().score;
        assert!(score(&played) > score(&base));
        assert_eq!(recommend(&lib, &["b1".into()], &[], 1).len(), 1);
    }

    #[test]
    fn empty_playlist_has_no_suggestions() {
        assert!(recommend(&library(), &[], &[], 10).is_empty());
        assert!(recommend(&library(), &["not-in-library".into()], &[], 10).is_empty());
    }

    #[test]
    fn credits_include_album_artist_and_featured() {
        let mut x = t("x", "Song (feat. Robyn)", "Royksopp", "", "", None);
        x.album_artist = "Various Artists".into();
        assert_eq!(credited(&x), vec!["royksopp".to_string(), "robyn".to_string()]);
    }
}
