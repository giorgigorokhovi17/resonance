//! Playlist matcher: maps external track references to local files, with an explained score.
//!
//! Tiers, in order. The first that decides wins:
//! 1. **Decision**: the user already decided for this reference (accept a file / missing / skip).
//! 2. **Path**: the location resolves to a library file, or a file on disk.
//! 3. **Path suffix**: the location's folder + file name match exactly one library file
//!    (playlists from a phone or another PC).
//! 4. **ISRC**, then 5. **MusicBrainz recording id**: exactly one library file carries it.
//! 6. **Fuzzy**: SQLite FTS5 narrows the library to candidates sharing words with the reference;
//!    each candidate is scored on title, artist set, version qualifiers and duration.
//!
//! A fuzzy match is auto-accepted only when the title matches exactly after normalisation, the
//! artists agree (≥ 0.9), the version qualifiers and drift markers are identical, both durations
//! are known and within ±3 s, the local file is at least 30 s long, the source allows it, and
//! exactly one candidate passes all of that. Everything else goes to review, or is reported as
//! unmatched with a reason. Wrong matches are worse than misses, so every rule errs towards
//! review.

pub mod normalize;

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;
use unicode_normalization::UnicodeNormalization;

use super::db::{self, MatchDecision};
use super::interchange::{is_url, normalize_isrc, resolve_location, TrackRef};
use super::model::Track;
use normalize::{artist_similarity, fold, readings, token_similarity, tokens, Reading, NORM_VERSION};

pub const DURATION_TOLERANCE_S: f64 = 3.0;
/// Files shorter than this (sound effects, voice notes) are never auto-accepted.
pub const MIN_AUTO_DURATION_S: f64 = 30.0;
const FTS_LIMIT: usize = 64;
/// Below this a candidate isn't worth showing.
const SHOW_MIN_SCORE: f64 = 0.35;
/// The best candidate must reach this to put a reference in review.
const REVIEW_MIN_SCORE: f64 = 0.5;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "camelCase")]
pub enum Tier {
    Decision,
    Path,
    PathSuffix,
    Isrc,
    Mbid,
    Fuzzy,
}

impl Tier {
    pub fn name(self) -> &'static str {
        match self {
            Tier::Decision => "decision",
            Tier::Path => "path",
            Tier::PathSuffix => "path-suffix",
            Tier::Isrc => "isrc",
            Tier::Mbid => "mbid",
            Tier::Fuzzy => "fuzzy",
        }
    }
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum Reason {
    /// Accepted.
    None,
    /// Nothing in the library looks like it.
    NoCandidate,
    /// Several files fit equally well.
    Ambiguous,
    DurationMismatch,
    /// Same song, different version (remix, edit, slowed, instrumental, …).
    VersionMismatch,
    /// Same recording apart from remaster / explicit-clean / album-single version: pick it in review.
    Drift,
    /// Probably right but can't be confirmed (no duration, inexact title or artist, short file,
    /// a source that never auto-accepts).
    LowConfidence,
    /// The user said it's not in the library.
    MarkedMissing,
    /// The user skipped it.
    Skipped,
}

impl Reason {
    pub fn name(self) -> &'static str {
        match self {
            Reason::None => "",
            Reason::NoCandidate => "no candidate",
            Reason::Ambiguous => "ambiguous",
            Reason::DurationMismatch => "duration mismatch",
            Reason::VersionMismatch => "version mismatch",
            Reason::Drift => "remaster/explicit difference",
            Reason::LowConfidence => "low confidence",
            Reason::MarkedMissing => "marked missing",
            Reason::Skipped => "skipped",
        }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    /// 0 for a file that exists on disk but isn't in the library.
    pub track_id: i64,
    pub path: String,
    pub title: String,
    pub artist: String,
    /// Seconds.
    pub duration: f64,
    /// 0..1.
    pub score: f64,
    /// Short signal summary: "title same · artist same · Δ 0.4 s".
    pub why: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MatchResult {
    pub ref_key: String,
    /// The local file, when accepted automatically or by an earlier decision.
    pub accepted: Option<Candidate>,
    pub tier: Option<Tier>,
    pub reason: Reason,
    /// One sentence for the user.
    pub explanation: String,
    /// Best candidates first (up to 3), for review.
    pub candidates: Vec<Candidate>,
}

impl MatchResult {
    /// Goes in the review queue: not accepted, not already decided, and there is something to pick.
    pub fn needs_review(&self) -> bool {
        self.accepted.is_none() && !matches!(self.reason, Reason::MarkedMissing | Reason::Skipped) && !self.candidates.is_empty()
    }
}

/// What a source may auto-accept.
#[derive(Clone, Copy, Debug)]
pub struct Policy {
    /// Fuzzy matches can be auto-accepted. Off for Spotify account data, which has no durations:
    /// the version can't be confirmed (decided 2026-10-18).
    pub fuzzy_auto: bool,
}

pub fn policy_for(source: &str) -> Policy {
    Policy { fuzzy_auto: source != "spotify-json" }
}

/// Stable identity of a reference, for decisions: a service id when there is one, otherwise a
/// hash of what describes it.
pub fn ref_key(r: &TrackRef) -> String {
    if let Some((svc, id)) = r.service_ids.iter().next() {
        return format!("{svc}:{id}");
    }
    let mut h = blake3::Hasher::new();
    let mut part = |s: &str| {
        h.update(s.as_bytes());
        h.update(b"\x1f");
    };
    part(&fold(&r.title));
    let mut artists: Vec<String> = r.artists.iter().map(|a| fold(a)).collect();
    artists.sort();
    part(&artists.join("\x1e"));
    part(&fold(&r.album));
    part(&r.duration_ms.map(|d| (d / 1000).to_string()).unwrap_or_default());
    part(r.isrc.as_deref().unwrap_or(""));
    part(r.mbid.as_deref().unwrap_or(""));
    part(&r.location.as_deref().map(fold_path).unwrap_or_default());
    format!("ref:{}", &h.finalize().to_hex()[..32])
}

/// Paths compare case-insensitively and NFC-normalised (one of the library's file names is NFD).
fn fold_path(p: &str) -> String {
    p.nfc().collect::<String>().replace('/', "\\").to_lowercase()
}

fn tail(path_fold: &str, n: usize) -> Option<String> {
    let parts: Vec<&str> = path_fold.split('\\').filter(|s| !s.is_empty()).collect();
    (parts.len() >= n).then(|| parts[parts.len() - n..].join("\\"))
}

struct Local {
    track: Track,
    readings: Vec<Reading>,
}

impl Local {
    fn candidate(&self, score: f64, why: String) -> Candidate {
        Candidate {
            track_id: self.track.id,
            path: self.track.path.clone(),
            title: self.track.title.clone(),
            artist: self.track.artist.clone(),
            duration: self.track.duration,
            score: (score * 1000.0).round() / 1000.0,
            why,
        }
    }
}

fn track_readings(t: &Track) -> Vec<Reading> {
    let artist = if t.artist.is_empty() { &t.album_artist } else { &t.artist };
    readings(&t.title, &[artist.clone()])
}

/// Bring the FTS candidate table up to date with the library: index new and re-read tracks
/// (and everything after a normalisation change), drop removed ones. Returns how many were
/// (re)indexed.
pub fn sync_fts(conn: &mut Connection) -> Result<usize> {
    let tx = conn.transaction()?;
    // Removed tracks, found through the plain state table (a NOT IN scan over the FTS table
    // itself is slow), then deleted from the FTS table one rowid at a time.
    let gone: Vec<i64> = tx
        .prepare("SELECT track_id FROM track_match_state WHERE track_id NOT IN (SELECT id FROM tracks)")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    {
        let mut del = tx.prepare_cached("DELETE FROM track_match_fts WHERE rowid = ?1")?;
        let mut del_state = tx.prepare_cached("DELETE FROM track_match_state WHERE track_id = ?1")?;
        for id in &gone {
            del.execute(params![id])?;
            del_state.execute(params![id])?;
        }
    }
    let stale: Vec<(i64, i64, String, String, String)> = tx
        .prepare(
            "SELECT t.id, t.mtime, t.title, t.artist, t.album_artist FROM tracks t
             LEFT JOIN track_match_state s ON s.track_id = t.id
             WHERE s.track_id IS NULL OR s.mtime != t.mtime OR s.norm != ?1 OR s.readings IS NULL",
        )?
        .query_map(params![NORM_VERSION], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
        .collect::<Result<_, _>>()?;
    {
        let mut del = tx.prepare_cached("DELETE FROM track_match_fts WHERE rowid = ?1")?;
        let mut ins = tx.prepare_cached("INSERT INTO track_match_fts(rowid, title, artist) VALUES(?1, ?2, ?3)")?;
        let mut st = tx.prepare_cached(
            "INSERT OR REPLACE INTO track_match_state(track_id, mtime, norm, readings) VALUES(?1, ?2, ?3, ?4)",
        )?;
        for (id, mtime, title, artist, album_artist) in &stale {
            let artist = if artist.is_empty() { album_artist } else { artist };
            let rs = readings(title, &[artist.clone()]);
            let mut words: Vec<String> = Vec::new();
            for r in &rs {
                words.extend(r.title_tokens.iter().cloned());
                words.extend(r.versions.iter().flat_map(|v| v.split(' ').map(str::to_string)));
            }
            words.dedup();
            del.execute(params![id])?;
            ins.execute(params![id, words.join(" "), tokens(artist).join(" ")])?;
            st.execute(params![id, mtime, NORM_VERSION, serde_json::to_string(&rs)?])?;
        }
    }
    tx.commit()?;
    Ok(stale.len())
}

/// Track ids for an FTS5 query, best bm25 first.
fn fts_ids(conn: &Connection, query: &str) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare_cached(
        "SELECT rowid FROM track_match_fts WHERE track_match_fts MATCH ?1 ORDER BY bm25(track_match_fts, 1.0, 0.5) LIMIT ?2",
    )?;
    let ids = stmt.query_map(params![query, FTS_LIMIT as i64], |row| row.get::<_, i64>(0))?;
    Ok(ids.collect::<Result<_, _>>()?)
}

/// Scores of one reference against one local track.
struct Fit {
    score: f64,
    title_exact: bool,
    artist: Option<f64>,
    versions_eq: bool,
    drift_eq: bool,
    ref_versions: String,
    local_versions: String,
    delta: Option<f64>,
    from_title: bool,
}

fn describe_versions(r: &Reading) -> String {
    let all: Vec<&str> = r.versions.iter().chain(r.drift.iter()).map(String::as_str).collect();
    if all.is_empty() { "no qualifier".into() } else { all.join(", ") }
}

fn fit(ref_readings: &[Reading], ref_duration_ms: Option<u64>, local: &Local) -> Fit {
    let mut best: Option<(f64, &Reading, &Reading, f64, Option<f64>)> = None;
    for a in ref_readings {
        for b in &local.readings {
            let title_sim = if !a.title_key.is_empty() && a.title_key == b.title_key {
                1.0
            } else {
                token_similarity(&a.title_tokens, &b.title_tokens)
            };
            let artist = artist_similarity(&a.artists, &b.artists);
            let mut s = 0.55 * title_sim + 0.45 * artist.unwrap_or(0.4);
            if a.versions != b.versions {
                s *= 0.6;
            }
            if a.drift != b.drift {
                s *= 0.9;
            }
            if best.as_ref().is_none_or(|(bs, ..)| s > *bs) {
                best = Some((s, a, b, title_sim, artist));
            }
        }
    }
    let (s, a, b, title_sim, artist) = best.expect("at least one reading on each side");
    let delta = ref_duration_ms.filter(|_| local.track.duration > 0.0).map(|ms| (ms as f64 / 1000.0 - local.track.duration).abs());
    let dur_score = match delta {
        None => 0.5,
        Some(d) if d <= DURATION_TOLERANCE_S => 1.0,
        Some(d) => (1.0 - (d - DURATION_TOLERANCE_S) / 12.0).max(0.0),
    };
    Fit {
        score: 0.85 * s + 0.15 * dur_score,
        title_exact: title_sim == 1.0,
        artist,
        versions_eq: a.versions == b.versions,
        drift_eq: a.drift == b.drift,
        ref_versions: describe_versions(a),
        local_versions: describe_versions(b),
        delta,
        from_title: a.from_title || b.from_title,
    }
}

impl Fit {
    fn why(&self) -> String {
        let mut parts = vec![if self.title_exact { "title same".to_string() } else { "title similar".to_string() }];
        parts.push(match self.artist {
            Some(a) if a >= 0.999 => "artist same".into(),
            Some(a) if a >= 0.9 => "artists overlap".into(),
            Some(a) => format!("artist differs ({:.0}%)", a * 100.0),
            None => "no artist to compare".into(),
        });
        if !self.versions_eq || !self.drift_eq {
            parts.push(format!("version: {} vs local {}", self.ref_versions, self.local_versions));
        }
        parts.push(match self.delta {
            Some(d) => format!("Δ {d:.1} s"),
            None => "no duration".into(),
        });
        if self.from_title {
            parts.push("artist read from an \"Artist - Title\" title".into());
        }
        parts.join(" · ")
    }

    fn auto_ok(&self, local: &Local, policy: Policy) -> bool {
        self.title_exact
            && self.artist.is_some_and(|a| a >= 0.9)
            && self.versions_eq
            && self.drift_eq
            && self.delta.is_some_and(|d| d <= DURATION_TOLERANCE_S)
            && local.track.duration >= MIN_AUTO_DURATION_S
            && policy.fuzzy_auto
    }
}

pub struct Matcher {
    locals: Vec<Local>,
    by_path: HashMap<String, usize>,
    by_tail2: HashMap<String, Vec<usize>>,
    by_name: HashMap<String, Vec<usize>>,
    by_isrc: HashMap<String, Vec<usize>>,
    by_mbid: HashMap<String, Vec<usize>>,
    by_id: HashMap<i64, usize>,
    decisions: HashMap<String, MatchDecision>,
}

impl Matcher {
    /// Sync the FTS table, then load the library and the user's decisions.
    pub fn open(conn: &mut Connection) -> Result<Self> {
        sync_fts(conn)?;
        let tracks = db::all_tracks(conn)?;
        let decisions = db::match_decisions(conn)?;
        let mut cached: HashMap<i64, Vec<Reading>> = HashMap::with_capacity(tracks.len());
        {
            let mut stmt = conn.prepare("SELECT track_id, readings FROM track_match_state WHERE norm = ?1")?;
            let rows = stmt.query_map(params![NORM_VERSION], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?)))?;
            for row in rows {
                let (id, json) = row?;
                if let Some(rs) = json.and_then(|j| serde_json::from_str(&j).ok()) {
                    cached.insert(id, rs);
                }
            }
        }
        Ok(Self::from_tracks(tracks, decisions, cached))
    }

    fn from_tracks(tracks: Vec<Track>, decisions: HashMap<String, MatchDecision>, mut cached: HashMap<i64, Vec<Reading>>) -> Self {
        let mut m = Matcher {
            locals: Vec::with_capacity(tracks.len()),
            by_path: HashMap::new(),
            by_tail2: HashMap::new(),
            by_name: HashMap::new(),
            by_isrc: HashMap::new(),
            by_mbid: HashMap::new(),
            by_id: HashMap::new(),
            decisions,
        };
        for t in tracks {
            let i = m.locals.len();
            let pf = fold_path(&t.path);
            m.by_path.insert(pf.clone(), i);
            if let Some(k) = tail(&pf, 2) {
                m.by_tail2.entry(k).or_default().push(i);
            }
            if let Some(k) = tail(&pf, 1) {
                m.by_name.entry(k).or_default().push(i);
            }
            if let Some(isrc) = &t.isrc {
                m.by_isrc.entry(isrc.clone()).or_default().push(i);
            }
            if let Some(mbid) = &t.mbid {
                m.by_mbid.entry(mbid.clone()).or_default().push(i);
            }
            m.by_id.insert(t.id, i);
            let readings = cached.remove(&t.id).unwrap_or_else(|| track_readings(&t));
            m.locals.push(Local { readings, track: t });
        }
        m
    }

    pub fn len(&self) -> usize {
        self.locals.len()
    }

    pub fn is_empty(&self) -> bool {
        self.locals.is_empty()
    }

    fn result(key: &str, accepted: Option<Candidate>, tier: Option<Tier>, reason: Reason, explanation: String, candidates: Vec<Candidate>) -> MatchResult {
        MatchResult { ref_key: key.to_string(), accepted, tier, reason, explanation, candidates }
    }

    /// Accept `local` from an identifier tier, unless the duration or length says otherwise.
    fn id_tier(&self, key: &str, r: &TrackRef, idxs: &[usize], tier: Tier, what: &str) -> MatchResult {
        let cands: Vec<Candidate> = idxs
            .iter()
            .map(|&i| {
                let l = &self.locals[i];
                let delta = r.duration_ms.map(|ms| (ms as f64 / 1000.0 - l.track.duration).abs());
                let why = match delta {
                    Some(d) => format!("same {what} · Δ {d:.1} s"),
                    None => format!("same {what}"),
                };
                l.candidate(1.0, why)
            })
            .collect();
        if idxs.len() > 1 {
            return Self::result(key, None, None, Reason::Ambiguous, format!("{} local files share this {what}.", idxs.len()), cands);
        }
        let l = &self.locals[idxs[0]];
        let c = cands.into_iter().next().expect("one candidate");
        if let Some(d) = r.duration_ms.map(|ms| (ms as f64 / 1000.0 - l.track.duration).abs()).filter(|d| *d > DURATION_TOLERANCE_S) {
            return Self::result(key, None, None, Reason::DurationMismatch, format!("Same {what}, but the durations differ by {d:.0} s."), vec![c]);
        }
        if l.track.duration < MIN_AUTO_DURATION_S {
            return Self::result(key, None, None, Reason::LowConfidence, format!("Same {what}, but the local file is only {:.0} s long.", l.track.duration), vec![c]);
        }
        let explanation = format!("Same {what}.");
        Self::result(key, Some(c), Some(tier), Reason::None, explanation, Vec::new())
    }

    /// Match one reference. `base` resolves relative locations (the playlist file's folder).
    pub fn match_ref(&self, conn: &Connection, r: &TrackRef, base: Option<&Path>, policy: Policy) -> Result<MatchResult> {
        let key = ref_key(r);
        let mut hints: Vec<Candidate> = Vec::new();
        // Why an earlier tier found something but couldn't accept it; it outranks fuzzy guesses.
        let mut pending: Option<(Reason, String)> = None;
        let mut note = String::new();

        // 1. An earlier decision.
        if let Some(d) = self.decisions.get(&key) {
            match d.decision.as_str() {
                "missing" => return Ok(Self::result(&key, None, Some(Tier::Decision), Reason::MarkedMissing, "You marked this as not in your library.".into(), Vec::new())),
                "skip" => return Ok(Self::result(&key, None, Some(Tier::Decision), Reason::Skipped, "You skipped this one.".into(), Vec::new())),
                _ => {
                    if let Some(p) = &d.path {
                        if let Some(&i) = self.by_path.get(&fold_path(p)) {
                            let c = self.locals[i].candidate(1.0, "your choice".into());
                            return Ok(Self::result(&key, Some(c), Some(Tier::Decision), Reason::None, "You chose this file.".into(), Vec::new()));
                        }
                        if Path::new(p).is_file() {
                            let c = Candidate { track_id: 0, path: p.clone(), title: String::new(), artist: String::new(), duration: 0.0, score: 1.0, why: "your choice".into() };
                            return Ok(Self::result(&key, Some(c), Some(Tier::Decision), Reason::None, "You chose this file.".into(), Vec::new()));
                        }
                        note = format!("The file you chose earlier ({p}) is gone. ");
                    }
                }
            }
        }

        // 2-3. Paths.
        if let Some(loc) = r.location.as_deref().filter(|l| !is_url(l)) {
            if let Some(abs) = resolve_location(loc, base.unwrap_or(Path::new("."))) {
                let pf = fold_path(&abs);
                if let Some(&i) = self.by_path.get(&pf) {
                    let c = self.locals[i].candidate(1.0, "same path".into());
                    return Ok(Self::result(&key, Some(c), Some(Tier::Path), Reason::None, "Same file path.".into(), Vec::new()));
                }
                if Path::new(&abs).is_file() {
                    let c = Candidate { track_id: 0, path: abs.clone(), title: r.title.clone(), artist: r.artists.join(", "), duration: 0.0, score: 1.0, why: "file exists".into() };
                    return Ok(Self::result(&key, Some(c), Some(Tier::Path), Reason::None, "The file exists at this path (it isn't in your library).".into(), Vec::new()));
                }
                let lf = fold_path(loc);
                if let Some(idxs) = tail(&lf, 2).and_then(|k| self.by_tail2.get(&k)) {
                    let res = self.id_tier(&key, r, idxs, Tier::PathSuffix, "folder and file name");
                    if res.accepted.is_some() || res.reason == Reason::Ambiguous {
                        return Ok(res);
                    }
                    pending = Some((res.reason, res.explanation));
                    hints.extend(res.candidates);
                } else if let Some(idxs) = tail(&lf, 1).and_then(|k| self.by_name.get(&k)) {
                    for &i in idxs.iter().take(3) {
                        hints.push(self.locals[i].candidate(0.6, "same file name, different folder".into()));
                    }
                    pending = Some((Reason::LowConfidence, "A file with the same name is in a different folder.".into()));
                }
            }
        }

        // 4-5. Identifiers.
        let isrc = r.isrc.as_deref().and_then(normalize_isrc);
        if let Some(idxs) = isrc.as_ref().and_then(|i| self.by_isrc.get(i)) {
            let res = self.id_tier(&key, r, idxs, Tier::Isrc, "ISRC");
            if res.accepted.is_some() || res.reason == Reason::Ambiguous {
                return Ok(res);
            }
            pending = Some((res.reason, res.explanation));
            hints.extend(res.candidates);
        }
        if let Some(idxs) = r.mbid.as_ref().and_then(|m| self.by_mbid.get(&m.to_ascii_lowercase())) {
            let res = self.id_tier(&key, r, idxs, Tier::Mbid, "MusicBrainz id");
            if res.accepted.is_some() || res.reason == Reason::Ambiguous {
                return Ok(res);
            }
            pending.get_or_insert((res.reason, res.explanation));
            hints.extend(res.candidates);
        }

        // 6. Fuzzy, over FTS candidates.
        let ref_readings = readings(&r.title, &r.artists);
        // Candidates: tracks containing every title word of some reading (cheap, and enough for
        // nearly every real match); if that finds little, any shared word, ranked by bm25.
        // With an artist, require one of its words too, so a one-word title ("Love") in a big
        // library doesn't drown the right track below the LIMIT.
        let mut ids: Vec<i64> = Vec::new();
        let artist_words: Vec<String> = r.artists.iter().flat_map(|a| tokens(a)).collect();
        for rd in &ref_readings {
            if !rd.title_tokens.is_empty() {
                let mut q = rd.title_tokens.iter().map(|w| format!("\"{w}\"")).collect::<Vec<_>>().join(" AND ");
                if !rd.from_title && !artist_words.is_empty() {
                    let a = artist_words.iter().map(|w| format!("\"{w}\"")).collect::<Vec<_>>().join(" OR ");
                    q = format!("({q}) AND ({a})");
                }
                ids.extend(fts_ids(conn, &q)?);
            }
        }
        ids.sort_unstable();
        ids.dedup();
        if ids.len() < 8 {
            let mut words: Vec<String> = Vec::new();
            for rd in &ref_readings {
                words.extend(rd.title_tokens.iter().cloned());
                words.extend(rd.versions.iter().flat_map(|v| v.split(' ').map(str::to_string)));
            }
            for a in &r.artists {
                words.extend(tokens(a));
            }
            words.sort();
            words.dedup();
            words.truncate(32);
            if !words.is_empty() {
                let q = words.iter().map(|w| format!("\"{w}\"")).collect::<Vec<_>>().join(" OR ");
                ids.extend(fts_ids(conn, &q)?);
                ids.sort_unstable();
                ids.dedup();
            }
        }
        let mut scored: Vec<(Fit, usize)> = Vec::new();
        {
            for id in ids {
                if let Some(&i) = self.by_id.get(&id) {
                    let mut f = fit(&ref_readings, r.duration_ms, &self.locals[i]);
                    // Different identifiers on both sides: almost certainly different recordings.
                    let l = &self.locals[i].track;
                    if (isrc.is_some() && l.isrc.is_some() && isrc != l.isrc) || (r.mbid.is_some() && l.mbid.is_some() && r.mbid != l.mbid) {
                        f.score *= 0.5;
                        f.drift_eq = false;
                        f.local_versions = format!("{} (different ISRC/MusicBrainz id)", f.local_versions);
                    }
                    scored.push((f, i));
                }
            }
        }
        scored.sort_by(|a, b| b.0.score.total_cmp(&a.0.score));
        // Both sides name artists and they don't overlap: a different song that shares words,
        // not a candidate (the "Artist - Title" readings already cover uploader-tagged files).
        let same_title_other_artist = scored.iter().any(|(f, _)| f.title_exact && f.artist.is_some_and(|a| a < 0.5));
        scored.retain(|(f, _)| f.artist.is_none_or(|a| a >= 0.5));

        let eligible: Vec<&(Fit, usize)> = scored.iter().filter(|(f, i)| f.auto_ok(&self.locals[*i], policy)).collect();
        let mut candidates: Vec<Candidate> = hints;
        for (f, i) in scored.iter().filter(|(f, _)| f.score >= SHOW_MIN_SCORE) {
            if !candidates.iter().any(|c| c.track_id == self.locals[*i].track.id) {
                candidates.push(self.locals[*i].candidate(f.score, f.why()));
            }
        }
        candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
        candidates.truncate(3);

        if eligible.len() == 1 {
            let (f, i) = eligible[0];
            let c = self.locals[*i].candidate(f.score, f.why());
            let explanation = format!("{note}Same title, artist and version; durations within {DURATION_TOLERANCE_S:.0} s ({}).", f.why());
            return Ok(Self::result(&key, Some(c), Some(Tier::Fuzzy), Reason::None, explanation, Vec::new()));
        }
        if eligible.len() > 1 {
            let cands = eligible.iter().take(3).map(|(f, i)| self.locals[*i].candidate(f.score, f.why())).collect();
            return Ok(Self::result(&key, None, None, Reason::Ambiguous, format!("{note}{} local files fit equally well.", eligible.len()), cands));
        }

        // Not accepted: say why. Path/identifier evidence first, then the best fuzzy candidate.
        if let Some((reason, why)) = pending {
            return Ok(Self::result(&key, None, None, reason, format!("{note}{why}"), candidates));
        }
        let Some((f, i)) = scored.first().filter(|(f, _)| f.score >= REVIEW_MIN_SCORE) else {
            // Same-title songs by other artists aren't offered for review: in a real library
            // ("Falling", "Heartbeat") they'd fill the queue with certain non-matches.
            if same_title_other_artist || candidates.is_empty() {
                let explanation = if same_title_other_artist {
                    format!("{note}Only songs with the same title by other artists.")
                } else {
                    format!("{note}No local song looks like this.")
                };
                return Ok(Self::result(&key, None, None, Reason::NoCandidate, explanation, Vec::new()));
            }
            let explanation = format!("{note}Nothing close enough; the nearest guesses are listed.");
            return Ok(Self::result(&key, None, None, Reason::LowConfidence, explanation, candidates));
        };
        let l = &self.locals[*i];
        let (reason, why) = if f.title_exact && f.artist.is_none_or(|a| a >= 0.9) && !f.versions_eq {
            (Reason::VersionMismatch, format!("Same song, different version: {} here, {} in your library.", f.ref_versions, f.local_versions))
        } else if f.title_exact && f.artist.is_none_or(|a| a >= 0.9) && !f.drift_eq {
            (Reason::Drift, format!("Same song apart from {} vs {}; pick it if that's fine.", f.ref_versions, f.local_versions))
        } else if let Some(d) = f.delta.filter(|d| *d > DURATION_TOLERANCE_S) {
            (Reason::DurationMismatch, format!("Looks like \"{}\", but the durations differ by {d:.0} s.", l.track.title))
        } else if l.track.duration < MIN_AUTO_DURATION_S {
            (Reason::LowConfidence, format!("Looks like \"{}\", but the local file is only {:.0} s long.", l.track.title, l.track.duration))
        } else if !policy.fuzzy_auto {
            (Reason::LowConfidence, "Looks right, but this source has no durations to confirm the version.".to_string())
        } else if f.delta.is_none() {
            (Reason::LowConfidence, "Looks right, but there's no duration to confirm the version.".to_string())
        } else if !f.title_exact {
            (Reason::LowConfidence, format!("The title is similar to \"{}\", not the same.", l.track.title))
        } else {
            (Reason::LowConfidence, format!("The artist differs from \"{}\".", l.track.artist))
        };
        Ok(Self::result(&key, None, None, reason, format!("{note}{why}"), candidates))
    }
}

/// CSV report of references that weren't accepted, with the reason and the best candidate.
pub fn unmatched_report(rows: &[(usize, TrackRef, MatchResult)]) -> String {
    use super::csv::text_cell;
    let mut s = String::from(
        "Position,Title,Artist,Album,Duration (ms),ISRC,Location,Reason,Explanation,Best candidate,Candidate score\r\n",
    );
    for (pos, r, m) in rows.iter().filter(|(_, _, m)| m.accepted.is_none()) {
        let best = m.candidates.first();
        let cells = [
            pos.to_string(),
            text_cell(&r.title),
            text_cell(&r.artists.join("; ")),
            text_cell(&r.album),
            r.duration_ms.map(|d| d.to_string()).unwrap_or_default(),
            r.isrc.clone().unwrap_or_default(),
            text_cell(r.location.as_deref().unwrap_or("")),
            m.reason.name().to_string(),
            text_cell(&m.explanation),
            text_cell(best.map(|c| c.path.as_str()).unwrap_or("")),
            best.map(|c| format!("{:.2}", c.score)).unwrap_or_default(),
        ];
        s.push_str(&cells.join(","));
        s.push_str("\r\n");
    }
    s
}

#[cfg(test)]
mod tests;
