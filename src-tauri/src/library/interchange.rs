//! Service-neutral playlist model shared by every importer and exporter, plus the text, path and
//! URI handling the file formats have in common.
//!
//! Parsers keep what the source says and don't guess: an `#EXTINF` display string such as
//! "Artist - Title" stays in `title` unless the format names the artist explicitly. Splitting it is
//! the matcher's job, because the same problem shows up in YouTube titles and uploader tags.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};
use serde::{Deserialize, Serialize};

use super::{csv, m3u, pls, wpl, xspf};

/// An ordered playlist from any source. Duplicates are kept: position is identity.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    pub name: String,
    /// Where it came from: "m3u", "pls", "xspf", "wpl" (later: "spotify", "youtube", …).
    pub source: String,
    /// Stable id within `source`, so re-importing updates instead of duplicating.
    pub source_id: Option<String>,
    pub items: Vec<PlaylistItem>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistItem {
    pub track: TrackRef,
    /// Unix seconds, when the source records it.
    pub added_at: Option<i64>,
}

/// A reference to a recording as some source describes it. Not necessarily a local file.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TrackRef {
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub duration_ms: Option<u64>,
    pub isrc: Option<String>,
    /// MusicBrainz recording id.
    pub mbid: Option<String>,
    /// Ids on other services, e.g. "spotify" → track id, "youtube" → video id.
    pub service_ids: BTreeMap<String, String>,
    /// File location as the source wrote it: a decoded path (file URIs are converted, separators
    /// are `\`), relative paths kept relative, or a non-file URL. See [`resolve_location`].
    pub location: Option<String>,
}

/// Playlist formats that can be read. CSV means our own export format (see `csv.rs`).
pub const PLAYLIST_EXTENSIONS: &[&str] = &["m3u", "m3u8", "pls", "xspf", "wpl", "csv"];

/// Read any supported playlist file. The format comes from the extension, or from the content
/// when the extension is unknown.
pub fn parse_file(file: &Path) -> Result<Playlist> {
    let bytes = std::fs::read(file).with_context(|| format!("cannot read {}", file.display()))?;
    let text = decode_text(&bytes);
    let stem = file.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Imported".into());
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let mut pl = match ext.as_str() {
        "m3u" | "m3u8" => m3u::parse_str(&text, &stem),
        "pls" => pls::parse_str(&text, &stem)?,
        "xspf" => xspf::parse_str(&text, &stem)?,
        "wpl" => wpl::parse_str(&text, &stem)?,
        "csv" => csv::parse_str(&text, &stem)?,
        _ => {
            let head = text.trim_start().get(..200.min(text.trim_start().len())).unwrap_or("").to_ascii_lowercase();
            if head.starts_with("[playlist]") {
                pls::parse_str(&text, &stem)?
            } else if head.contains("<?wpl") || head.contains("<smil") {
                wpl::parse_str(&text, &stem)?
            } else if head.contains("<playlist") {
                xspf::parse_str(&text, &stem)?
            } else {
                m3u::parse_str(&text, &stem)
            }
        }
    };
    if pl.source_id.is_none() {
        pl.source_id = Some(file_source_id(file));
    }
    Ok(pl)
}

/// Re-importing the same file should update its playlist, so file imports are keyed by their
/// normalized absolute path (case-insensitive, as Windows paths are).
pub fn file_source_id(file: &Path) -> String {
    let abs = std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf());
    normalize(&abs).to_string_lossy().to_lowercase()
}

/// What an import did.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImportOutcome {
    pub id: i64,
    pub name: String,
    /// Entries stored in the playlist.
    pub total: usize,
    /// Stored entries whose file isn't on this PC.
    pub missing: usize,
    /// Entries left out: streams and web links, which can't be played offline.
    pub skipped: usize,
    /// An earlier import of the same source was updated instead of creating a new playlist.
    pub updated: bool,
    /// Entries matched to a file, by tier ("path", "path-suffix", "isrc", "mbid", "fuzzy",
    /// "decision").
    pub matched: BTreeMap<String, usize>,
    /// Entries waiting for review (a candidate exists but couldn't be accepted automatically).
    pub review: usize,
}

/// Store a parsed playlist. Every entry goes through the matcher; an accepted match is stored
/// as that file's path, anything else as its resolved location (shown as missing) or, without a
/// location, as an `unmatched:<ref key>` placeholder. The reference itself is kept (by ref key)
/// so it can be reviewed and re-matched later. Re-importing the same source replaces the items
/// and keeps the playlist's name, description and photo, since the user may have edited them.
pub fn import_into_db(conn: &mut rusqlite::Connection, pl: &Playlist, base: &Path) -> Result<ImportOutcome> {
    use super::db;
    use super::matcher::{policy_for, Matcher};
    let matcher = Matcher::open(conn)?;
    let policy = policy_for(&pl.source);
    let mut items: Vec<(String, Option<String>)> = Vec::new();
    let mut refs: Vec<(String, String)> = Vec::new();
    let (mut skipped, mut review) = (0, 0);
    let mut matched: BTreeMap<String, usize> = BTreeMap::new();
    for item in &pl.items {
        // Store locations resolved, so later re-matching doesn't need the playlist's folder.
        let mut r = item.track.clone();
        let is_stream = r.location.as_deref().is_some_and(is_url);
        if let Some(loc) = r.location.as_deref().filter(|l| !is_url(l)) {
            r.location = resolve_location(loc, base);
        }
        let res = matcher.match_ref(conn, &r, None, policy)?;
        let path = match (&res.accepted, &r.location) {
            (Some(c), _) => {
                *matched.entry(res.tier.map(|t| t.name()).unwrap_or("").to_string()).or_default() += 1;
                c.path.clone()
            }
            _ if is_stream => {
                skipped += 1;
                continue;
            }
            (None, Some(loc)) => loc.clone(),
            (None, None) => format!("unmatched:{}", res.ref_key),
        };
        review += res.needs_review() as usize;
        refs.push((res.ref_key.clone(), serde_json::to_string(&r)?));
        items.push((path, Some(res.ref_key)));
    }
    if items.is_empty() {
        anyhow::bail!(if skipped > 0 {
            "The playlist only has streams or web links, which Undertone can't play."
        } else {
            "The playlist file has no entries."
        });
    }
    let missing = items.iter().filter(|(p, _)| !Path::new(p).is_file()).count();
    let existing = match &pl.source_id {
        Some(sid) => db::playlist_by_source(conn, &pl.source, sid)?,
        None => None,
    };
    let (id, name, updated) = match existing {
        Some((id, name)) => (id, name, true),
        None => {
            let id = db::create_playlist(conn, &pl.name)?;
            if let Some(sid) = &pl.source_id {
                db::set_playlist_source(conn, id, &pl.source, sid)?;
            }
            (id, pl.name.clone(), false)
        }
    };
    db::put_match_refs(conn, &refs)?;
    db::set_playlist_items(conn, id, &items)?;
    Ok(ImportOutcome { id, name, total: items.len(), missing, skipped, updated, matched, review })
}

/// One imported reference in a playlist, with what the matcher says about it now.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ReviewItem {
    /// 1-based positions in the playlist (a reference can appear more than once).
    pub positions: Vec<usize>,
    pub reference: TrackRef,
    pub result: super::matcher::MatchResult,
}

/// The review screen's data for one playlist.
#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistReview {
    /// Waiting for a decision: not accepted, with at least one candidate.
    pub pending: Vec<ReviewItem>,
    /// Decided by the user (accepted, missing or skipped), for undo.
    pub decided: Vec<ReviewItem>,
    /// Accepted automatically.
    pub matched: usize,
    /// Nothing in the library looks like them (no candidate to offer).
    pub unmatched: usize,
}

/// Group a playlist's imported entries by reference and sort them into the review screen's lists.
pub fn playlist_review(conn: &mut rusqlite::Connection, id: i64) -> Result<PlaylistReview> {
    use super::matcher::{Reason, Tier};
    let mut by_key: Vec<ReviewItem> = Vec::new();
    for (pos, r, res) in rematch_playlist(conn, id)? {
        match by_key.iter_mut().find(|i| i.result.ref_key == res.ref_key) {
            Some(item) => item.positions.push(pos),
            None => by_key.push(ReviewItem { positions: vec![pos], reference: r, result: res }),
        }
    }
    let mut out = PlaylistReview::default();
    for item in by_key {
        let m = &item.result;
        if m.tier == Some(Tier::Decision) || matches!(m.reason, Reason::MarkedMissing | Reason::Skipped) {
            out.decided.push(item);
        } else if m.accepted.is_some() {
            out.matched += 1;
        } else if m.needs_review() {
            out.pending.push(item);
        } else {
            out.unmatched += 1;
        }
    }
    Ok(out)
}

/// Record the user's decision for a reference and apply it to every playlist that has it.
/// `decision`: "accept" (with `path`), "missing", "skip", or "clear" to undo an earlier decision.
/// Returns the number of playlists whose items changed.
pub fn decide(conn: &mut rusqlite::Connection, key: &str, decision: &str, path: Option<&str>) -> Result<usize> {
    use super::db;
    use super::matcher::{policy_for, Matcher};
    use rusqlite::OptionalExtension;
    match decision {
        "accept" => {
            let p = path.ok_or_else(|| anyhow::anyhow!("choose a file to use"))?;
            if !Path::new(p).is_file() {
                anyhow::bail!("that file doesn't exist: {p}");
            }
            db::set_match_decision(conn, key, "accept", Some(p))?;
        }
        "missing" | "skip" => db::set_match_decision(conn, key, decision, None)?,
        "clear" => db::clear_match_decision(conn, key)?,
        other => anyhow::bail!("unknown decision {other:?}"),
    }
    // Re-match with the decision in place; the item's path follows the result.
    let Some(json) = db::match_ref_json(conn, key)? else { return Ok(0) };
    let r: TrackRef = serde_json::from_str(&json)?;
    let source: Option<String> = conn
        .query_row(
            "SELECT p.source FROM playlists p JOIN playlist_items i ON i.playlist_id = p.id WHERE i.ref_key = ?1 LIMIT 1",
            [key],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    let matcher = Matcher::open(conn)?;
    let res = matcher.match_ref(conn, &r, None, policy_for(source.as_deref().unwrap_or("")))?;
    let new_path = match (&res.accepted, &r.location) {
        (Some(c), _) => c.path.clone(),
        (None, Some(loc)) => loc.clone(),
        (None, None) => format!("unmatched:{key}"),
    };
    Ok(db::set_path_for_ref(conn, key, &new_path)?)
}

/// Re-run the matcher over a playlist's imported entries (with the current library and
/// decisions). Returns (position, reference, result) for every entry that came from an import.
pub fn rematch_playlist(conn: &mut rusqlite::Connection, id: i64) -> Result<Vec<(usize, TrackRef, super::matcher::MatchResult)>> {
    use super::db;
    use super::matcher::{policy_for, Matcher};
    let source = db::playlists(conn)?.into_iter().find(|p| p.id == id).and_then(|p| p.source).unwrap_or_default();
    let policy = policy_for(&source);
    let matcher = Matcher::open(conn)?;
    let mut out = Vec::new();
    for (pos, (_, key)) in db::playlist_items(conn, id)?.into_iter().enumerate() {
        let Some(key) = key else { continue };
        let Some(json) = db::match_ref_json(conn, &key)? else { continue };
        let r: TrackRef = serde_json::from_str(&json)?;
        let res = matcher.match_ref(conn, &r, None, policy)?;
        out.push((pos + 1, r, res));
    }
    Ok(out)
}

// ---- text -------------------------------------------------------------------------------------

/// Decode playlist bytes: UTF-8/UTF-16 by BOM, otherwise UTF-8, otherwise Windows-1252 (what
/// legacy Windows .m3u/.pls files use).
pub fn decode_text(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    let utf16 = |rest: &[u8], le: bool| {
        let units: Vec<u16> = rest
            .chunks_exact(2)
            .map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) })
            .collect();
        String::from_utf16_lossy(&units)
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, true);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, false);
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| cp1252(b)).collect(),
    }
}

fn cp1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž', '\u{8F}', '\u{90}', '‘', '’',
        '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}', 'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9F => HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

// ---- locations --------------------------------------------------------------------------------

/// `scheme://…`, but not a drive letter (`C:\`), which has a one-letter "scheme".
pub fn is_url(s: &str) -> bool {
    match s.find(':') {
        Some(i) if i >= 2 => {
            s[..i].chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
                && s.as_bytes()[0].is_ascii_alphabetic()
                && (s[i + 1..].starts_with("//") || s[..i].eq_ignore_ascii_case("spotify"))
        }
        _ => false,
    }
}

/// Convert a `file:` URI to a path: `file:///C:/a%20b` → `C:\a b`, `file://localhost/C:/x` →
/// `C:\x`, `file://server/share/x` → `\\server\share\x`, `file:///storage/x` → `\storage\x`.
pub fn file_uri_to_path(uri: &str) -> Option<String> {
    let lower = uri.get(..5)?.to_ascii_lowercase();
    if lower != "file:" {
        return None;
    }
    let rest = &uri[5..];
    let decoded = |s: &str| percent_decode_str(s).decode_utf8_lossy().replace('/', "\\");
    let path = if let Some(after) = rest.strip_prefix("//") {
        let (host, path) = after.split_at(after.find('/').unwrap_or(after.len()));
        if host.is_empty() || host.eq_ignore_ascii_case("localhost") {
            local_uri_path(&decoded(path))
        } else {
            format!("\\\\{}{}", decoded(host), decoded(path))
        }
    } else {
        local_uri_path(&decoded(rest))
    };
    Some(path)
}

/// `\C:\x` → `C:\x`; `\storage\x` stays rooted.
fn local_uri_path(p: &str) -> String {
    let b = p.as_bytes();
    if b.len() >= 3 && b[0] == b'\\' && b[1].is_ascii_alphabetic() && (b[2] == b':' || b[2] == b'|') {
        format!("{}:{}", &p[1..2], &p[3..])
    } else {
        p.to_string()
    }
}

/// Canonical form of a location written as a plain path or URI: file URIs decoded, separators
/// `\`, a leading `.\` dropped. Other URLs are kept as written.
pub fn canonical_location(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(p) = file_uri_to_path(raw) {
        return Some(p);
    }
    if is_url(raw) {
        return Some(raw.to_string());
    }
    let p = raw.replace('/', "\\");
    let p = p.strip_prefix(".\\").map(str::to_string).unwrap_or(p);
    Some(p)
}

fn has_percent_escape(s: &str) -> bool {
    let b = s.as_bytes();
    (0..b.len().saturating_sub(2)).any(|i| b[i] == b'%' && b[i + 1].is_ascii_hexdigit() && b[i + 2].is_ascii_hexdigit())
}

/// Resolve a canonical location to an absolute local path, or `None` for URLs. Relative paths
/// resolve against `base` (the playlist's folder). Some writers percent-encode plain paths
/// (`01%20Song.mp3`): the decoded form is used when it exists on disk, or when the raw one doesn't
/// and the entry looks encoded (no literal spaces). A real file named `100%AB.mp3` still resolves.
/// Rooted paths without a drive (`\storage\emulated\0\…`, from a phone) are returned as they are:
/// they can't be found here, and the matcher's suffix tier works on them.
pub fn resolve_location(loc: &str, base: &Path) -> Option<String> {
    if is_url(loc) {
        return None;
    }
    let to_abs = |s: &str| -> PathBuf {
        let p = Path::new(s);
        if p.is_absolute() || s.starts_with("\\\\") || s.starts_with('\\') {
            p.to_path_buf()
        } else {
            base.join(p)
        }
    };
    let raw = normalize(&to_abs(loc));
    if has_percent_escape(loc) {
        let dec = percent_decode_str(loc).decode_utf8_lossy().to_string();
        let decoded = normalize(&to_abs(&dec));
        if !raw.exists() && (decoded.exists() || !loc.contains(' ')) {
            return Some(decoded.to_string_lossy().to_string());
        }
    }
    Some(raw.to_string_lossy().to_string())
}

/// Resolve `.`/`..` without touching the filesystem (canonicalize would add a `\\?\` prefix
/// and fail for missing files).
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The location to write for `path`: relative to `base` when the file is inside it.
pub fn relative_to(path: &str, base: Option<&Path>) -> String {
    base.and_then(|b| Path::new(path).strip_prefix(b).ok())
        .map(|r| r.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

/// Characters escaped in URI paths. Non-ASCII is always percent-encoded as UTF-8.
const URI_PATH: &AsciiSet =
    &CONTROLS.add(b' ').add(b'"').add(b'#').add(b'%').add(b'<').add(b'>').add(b'?').add(b'[').add(b'\\').add(b']')
        .add(b'^').add(b'`').add(b'{').add(b'|').add(b'}').add(b';');

/// Location from a URI-based format (XSPF): file URIs and relative URIs are percent-decoded,
/// other URLs are kept.
pub fn uri_to_location(uri: &str) -> Option<String> {
    let uri = uri.trim();
    if uri.is_empty() {
        return None;
    }
    if let Some(p) = file_uri_to_path(uri) {
        return Some(p);
    }
    if is_url(uri) {
        return Some(uri.to_string());
    }
    let p = percent_decode_str(uri).decode_utf8_lossy().replace('/', "\\");
    Some(p.strip_prefix(".\\").map(str::to_string).unwrap_or(p))
}

/// Inverse of [`uri_to_location`] for URI-based formats (XSPF): `C:\a b` → `file:///C:/a%20b`,
/// `\\srv\share\x` → `file://srv/share/x`, a relative path → a relative URI. URLs pass through.
pub fn location_to_uri(loc: &str) -> String {
    if is_url(loc) {
        return loc.to_string();
    }
    let enc = |s: &str| s.split('\\').map(|seg| utf8_percent_encode(seg, URI_PATH).to_string()).collect::<Vec<_>>().join("/");
    let b = loc.as_bytes();
    if let Some(unc) = loc.strip_prefix("\\\\") {
        format!("file://{}", enc(unc))
    } else if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        format!("file:///{}:{}", &loc[..1], enc(&loc[2..]))
    } else if loc.starts_with('\\') {
        format!("file://{}", enc(loc))
    } else {
        enc(loc)
    }
}

// ---- identifiers ------------------------------------------------------------------------------

/// Recognise service/track identifiers written as URIs or URLs. Returns (service, id).
pub fn service_id_from_uri(uri: &str) -> Option<(&'static str, String)> {
    let u = uri.trim();
    if let Some(id) = u.strip_prefix("spotify:track:") {
        return Some(("spotify", id.to_string()));
    }
    for prefix in ["https://open.spotify.com/track/", "http://open.spotify.com/track/"] {
        if let Some(rest) = u.strip_prefix(prefix) {
            return Some(("spotify", rest.split(['?', '/']).next()?.to_string()));
        }
    }
    for prefix in ["https://www.youtube.com/watch?", "https://youtube.com/watch?", "https://music.youtube.com/watch?", "https://m.youtube.com/watch?"] {
        if let Some(q) = u.strip_prefix(prefix) {
            let v = q.split('&').find_map(|kv| kv.strip_prefix("v="))?;
            return Some(("youtube", v.to_string()));
        }
    }
    if let Some(rest) = u.strip_prefix("https://youtu.be/") {
        return Some(("youtube", rest.split(['?', '/']).next()?.to_string()));
    }
    None
}

/// MusicBrainz recording id from `https://musicbrainz.org/recording/<uuid>`.
pub fn mbid_from_uri(uri: &str) -> Option<String> {
    let u = uri.trim();
    let rest = u.strip_prefix("https://musicbrainz.org/recording/").or_else(|| u.strip_prefix("http://musicbrainz.org/recording/"))?;
    let id = rest.split(['?', '/', '#']).next()?;
    (id.len() == 36).then(|| id.to_ascii_lowercase())
}

/// Canonical ISRC (12 characters, upper case, no hyphens) when `s` is a valid one. Tags sometimes
/// hold barcodes or catalogue numbers in the ISRC field; those are rejected.
pub fn normalize_isrc(s: &str) -> Option<String> {
    let s: String = s.trim().chars().filter(|c| *c != '-').collect::<String>().to_ascii_uppercase();
    let ok = s.len() == 12
        && s[..2].chars().all(|c| c.is_ascii_alphabetic())
        && s[2..5].chars().all(|c| c.is_ascii_alphanumeric())
        && s[5..].chars().all(|c| c.is_ascii_digit());
    ok.then_some(s)
}

/// A playlist entry for a library track, for exporters.
pub fn track_ref_from_track(t: &super::model::Track) -> TrackRef {
    let artist = if t.artist.is_empty() { &t.album_artist } else { &t.artist };
    TrackRef {
        title: t.title.clone(),
        artists: if artist.is_empty() { Vec::new() } else { vec![artist.clone()] },
        album: t.album.clone(),
        duration_ms: (t.duration > 0.0).then(|| (t.duration * 1000.0).round() as u64),
        isrc: t.isrc.clone(),
        mbid: t.mbid.clone(),
        location: Some(t.path.clone()),
        ..Default::default()
    }
}

/// A minimal XML tree for the small playlist formats (XSPF, WPL). Names are local (namespace
/// prefixes dropped) and compared case-insensitively, because WPL files in the wild vary.
pub mod xml {
    use anyhow::{anyhow, Result};
    use quick_xml::events::Event;
    use quick_xml::Reader;

    #[derive(Debug, Default)]
    pub struct Element {
        pub name: String,
        pub attrs: Vec<(String, String)>,
        pub children: Vec<Element>,
        pub text: String,
    }

    impl Element {
        pub fn child(&self, name: &str) -> Option<&Element> {
            self.children.iter().find(|c| c.name.eq_ignore_ascii_case(name))
        }
        pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
            self.children.iter().filter(move |c| c.name.eq_ignore_ascii_case(name))
        }
        pub fn attr(&self, name: &str) -> Option<&str> {
            self.attrs.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
        }
        /// Trimmed text of the first child called `name`, if non-empty.
        pub fn text_of(&self, name: &str) -> Option<String> {
            self.child(name).map(|c| c.text.trim().to_string()).filter(|s| !s.is_empty())
        }
    }

    fn local(name: &str) -> String {
        name.rsplit(':').next().unwrap_or("").to_string()
    }

    pub fn parse(text: &str) -> Result<Element> {
        let mut reader = Reader::from_str(text.trim_start_matches('\u{feff}'));
        reader.config_mut().trim_text(false);
        let mut stack: Vec<Element> = vec![Element::default()]; // synthetic document node
        loop {
            let ev = reader.read_event().map_err(|e| anyhow!("invalid XML at byte {}: {e}", reader.buffer_position()))?;
            let element = |e: &quick_xml::events::BytesStart| Element {
                name: local(e.local_name().as_ref()),
                attrs: e
                    .attributes()
                    .flatten()
                    .map(|a| (local(a.key.local_name().as_ref()), a.normalized_value(quick_xml::XmlVersion::Implicit1_0).map(|v| v.into_owned()).unwrap_or_default()))
                    .collect(),
                ..Default::default()
            };
            match ev {
                Event::Start(e) => stack.push(element(&e)),
                Event::Empty(e) => {
                    let el = element(&e);
                    stack.last_mut().expect("document node").children.push(el);
                }
                Event::End(_) => {
                    if stack.len() > 1 {
                        let el = stack.pop().expect("open element");
                        stack.last_mut().expect("document node").children.push(el);
                    }
                }
                Event::Text(t) => stack.last_mut().expect("node").text.push_str(&t.xml10_content()),
                Event::CData(t) => stack.last_mut().expect("node").text.push_str(&t.xml10_content()),
                Event::GeneralRef(r) => {
                    let ch = match r.resolve_char_ref() {
                        Ok(Some(c)) => Some(c),
                        _ => match &*r.xml10_content() {
                            "amp" => Some('&'),
                            "lt" => Some('<'),
                            "gt" => Some('>'),
                            "quot" => Some('"'),
                            "apos" => Some('\''),
                            _ => None,
                        },
                    };
                    let node = stack.last_mut().expect("node");
                    match ch {
                        Some(c) => node.text.push(c),
                        None => node.text.push_str(&format!("&{};", r.xml10_content())),
                    }
                }
                Event::Eof => break,
                _ => {}
            }
        }
        // Unclosed elements: keep what was read.
        while stack.len() > 1 {
            let el = stack.pop().expect("open element");
            stack.last_mut().expect("document node").children.push(el);
        }
        let doc = stack.pop().expect("document node");
        doc.children.into_iter().next().ok_or_else(|| anyhow!("no XML root element"))
    }

    /// Escape text or an attribute value.
    pub fn esc(s: &str) -> String {
        quick_xml::escape::escape(s).into_owned()
    }
}

#[cfg(test)]
pub mod testgen {
    //! proptest strategies for round-trip tests. Strings are trimmed, single-line and non-empty
    //! where a format can't tell empty from missing; each format's test projects the model onto
    //! what it can represent.
    use super::*;
    use proptest::prelude::*;

    /// Text with the characters that break naive writers: XML specials, quotes, `%`, `#`, `;`
    /// (real artist tags contain "; "), `=`, non-ASCII from several scripts.
    pub fn text() -> impl Strategy<Value = String> {
        proptest::string::string_regex(r"[A-Za-z0-9 &<>'\x22%#,;=.()\[\]!?éüñ€’Мало тебя日本-]{1,24}")
            .unwrap()
            .prop_map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
            .prop_filter("non-empty", |s| !s.is_empty() && !s.starts_with('#'))
    }

    pub fn segment() -> impl Strategy<Value = String> {
        proptest::string::string_regex(r"[A-Za-z0-9 _&'()%#,;=$!.éüМ日-]{1,12}")
            .unwrap()
            .prop_map(|s| s.trim().trim_end_matches('.').to_string())
            .prop_filter("usable segment", |s| !s.is_empty() && s != "." && s != ".." && !s.starts_with('#'))
    }

    pub fn location() -> impl Strategy<Value = String> {
        let file = (segment(), prop_oneof![Just("flac"), Just("mp3"), Just("opus"), Just("m4a")]).prop_map(|(s, e)| format!("{s}.{e}"));
        (prop_oneof![Just("C:\\Music\\".to_string()), Just("D:\\".to_string()), Just(String::new()), Just("\\\\nas\\share\\".to_string())],
         proptest::collection::vec(segment(), 0..3), file)
            .prop_map(|(root, dirs, f)| {
                let mut p = root;
                for d in dirs {
                    p.push_str(&d);
                    p.push('\\');
                }
                p.push_str(&f);
                p
            })
    }

    pub fn track() -> impl Strategy<Value = TrackRef> {
        (
            text(),
            proptest::collection::vec(text(), 0..3),
            proptest::option::of(text()),
            proptest::option::of(1_000u64..3_600_000),
            proptest::option::of("[A-Z]{2}[A-Z0-9]{3}[0-9]{7}"),
            proptest::option::of("[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"),
            location(),
        )
            .prop_map(|(title, artists, album, duration_ms, isrc, mbid, loc)| TrackRef {
                title,
                artists,
                album: album.unwrap_or_default(),
                duration_ms,
                isrc,
                mbid,
                service_ids: BTreeMap::new(),
                location: Some(loc),
            })
    }

    pub fn playlist(source: &'static str) -> impl Strategy<Value = Playlist> {
        (text(), proptest::collection::vec((track(), proptest::option::of(1_000_000_000i64..2_000_000_000)), 0..12)).prop_map(
            move |(name, items)| Playlist {
                name,
                source: source.into(),
                source_id: None,
                items: items.into_iter().map(|(track, added_at)| PlaylistItem { track, added_at }).collect(),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reimport_updates_instead_of_duplicating() {
        let dir = std::env::temp_dir().join(format!("ic-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("Mix.m3u8");
        std::fs::write(&f, "#EXTM3U\r\na.flac\r\nhttp://radio/x\r\nb.flac\r\n").unwrap();
        let mut conn = super::super::db::open(Path::new(":memory:")).unwrap();

        let first = import_into_db(&mut conn, &parse_file(&f).unwrap(), &dir).unwrap();
        assert_eq!((first.total, first.missing, first.skipped, first.updated), (2, 2, 1, false));
        super::super::db::rename_playlist(&conn, first.id, "My mix").unwrap();

        // Same file, edited, and reached through a differently-cased path: same playlist.
        std::fs::write(&f, "#EXTM3U\r\nb.flac\r\na.flac\r\na.flac\r\n").unwrap();
        let other_case = PathBuf::from(f.to_string_lossy().to_uppercase());
        let second = import_into_db(&mut conn, &parse_file(&other_case).unwrap(), &dir).unwrap();
        assert_eq!((second.id, second.name.as_str(), second.total, second.updated), (first.id, "My mix", 3, true));
        let paths = super::super::db::playlist_paths(&conn, first.id).unwrap();
        let names: Vec<_> = paths.iter().map(|p| Path::new(p).file_name().unwrap().to_string_lossy().to_string()).collect();
        assert_eq!(names, ["b.flac", "a.flac", "a.flac"], "order and duplicates kept");
        assert_eq!(super::super::db::playlists(&conn).unwrap().len(), 1);

        // A different file with the same name is a different playlist.
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("Mix.m3u8"), "c.flac\r\n").unwrap();
        let third = import_into_db(&mut conn, &parse_file(&sub.join("Mix.m3u8")).unwrap(), &sub).unwrap();
        assert!(!third.updated && third.id != first.id);

        std::fs::write(&f, "http://only/streams\r\n").unwrap();
        assert!(import_into_db(&mut conn, &parse_file(&f).unwrap(), &dir).unwrap_err().to_string().contains("streams"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_runs_the_matcher_and_keeps_references_through_edits() {
        use super::super::{db, model::Track};
        let dir = std::env::temp_dir().join(format!("ic-match-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut conn = db::open(Path::new(":memory:")).unwrap();
        let folder = db::add_folder(&conn, r"C:\Music").unwrap();
        let t = |p: &str, title: &str, artist: &str, d: f64| {
            (Track { path: p.into(), title: title.into(), artist: artist.into(), duration: d, mtime: 1, ..Default::default() }, r"C:\Music".to_string())
        };
        db::upsert_tracks(&mut conn, folder, &[
            t(r"C:\Music\MQX\01 - Falling.flac", "Falling", "MQX", 154.6),
            t(r"C:\Music\Swimming Pools (Drank) [Extended].flac", "Swimming Pools (Drank) [Extended]", "Kendrick Lamar", 313.8),
        ]).unwrap();
        // A phone playlist: paths that don't exist here, EXTINF display strings.
        let f = dir.join("phone.m3u8");
        std::fs::write(&f, "#EXTM3U\r\n\
            #EXTINF:155,MQX - Falling\r\n/storage/emulated/0/Music/MQX/01 - Falling.flac\r\n\
            #EXTINF:221,Kendrick Lamar - Swimming Pools (Drank)\r\n/storage/emulated/0/Music/KL/Swimming Pools.mp3\r\n\
            #EXTINF:314,Kendrick Lamar - Swimming Pools (Drank) - Extended Version\r\n/storage/emulated/0/x.mp3\r\n").unwrap();
        let out = import_into_db(&mut conn, &parse_file(&f).unwrap(), &dir).unwrap();
        assert_eq!(out.matched.get("path-suffix"), Some(&1), "{out:?}");
        assert_eq!(out.matched.get("fuzzy"), Some(&1), "{out:?}");
        assert_eq!((out.total, out.review), (3, 1), "the original Swimming Pools waits for review: {out:?}");
        let items = db::playlist_items(&conn, out.id).unwrap();
        assert_eq!(items[0].0, r"C:\Music\MQX\01 - Falling.flac");
        assert!(items[1].0.ends_with(r"KL\Swimming Pools.mp3"), "unmatched keeps its own location");
        assert_eq!(items[2].0, r"C:\Music\Swimming Pools (Drank) [Extended].flac");
        assert!(items.iter().all(|(_, k)| k.is_some()));

        // Reordering in the app keeps each item's reference.
        let reversed: Vec<String> = items.iter().rev().map(|(p, _)| p.clone()).collect();
        db::set_playlist_paths(&mut conn, out.id, &reversed).unwrap();
        let after = db::playlist_items(&conn, out.id).unwrap();
        assert_eq!(after.iter().map(|(_, k)| k.clone()).collect::<Vec<_>>(), items.iter().rev().map(|(_, k)| k.clone()).collect::<Vec<_>>());

        // Re-matching from the stored references gives the same answers without the playlist file.
        let rows = rematch_playlist(&mut conn, out.id).unwrap();
        assert_eq!(rows.iter().filter(|(_, _, m)| m.accepted.is_some()).count(), 2);
        assert_eq!(rows.iter().find(|(_, _, m)| m.accepted.is_none()).unwrap().2.reason, super::super::matcher::Reason::VersionMismatch);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn review_and_decisions_apply_to_every_playlist_and_undo() {
        use super::super::{db, matcher::Reason, model::Track};
        let dir = std::env::temp_dir().join(format!("ic-review-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ext = dir.join("Swimming Pools (Drank) [Extended].flac");
        let inst = dir.join("Swimming Pools (Drank) (instrumental).flac");
        std::fs::write(&ext, b"").unwrap();
        std::fs::write(&inst, b"").unwrap();
        let mut conn = db::open(Path::new(":memory:")).unwrap();
        let folder = db::add_folder(&conn, &dir.to_string_lossy()).unwrap();
        let t = |p: &Path, title: &str, d: f64| {
            (Track { path: p.to_string_lossy().into(), title: title.into(), artist: "Kendrick Lamar".into(), duration: d, mtime: 1, ..Default::default() }, dir.to_string_lossy().to_string())
        };
        db::upsert_tracks(&mut conn, folder, &[t(&ext, "Swimming Pools (Drank) [Extended]", 313.8), t(&inst, "Swimming Pools (Drank) (instrumental)", 247.0)]).unwrap();

        // The same reference (the original, not in the library) in two playlists, twice in one.
        let body = "#EXTM3U\r\n#EXTINF:221,Kendrick Lamar - Swimming Pools (Drank)\r\nC:\\phone\\pools.mp3\r\n\
                    #EXTINF:314,Kendrick Lamar - Swimming Pools (Drank) [Extended]\r\nC:\\phone\\x.mp3\r\n\
                    #EXTINF:221,Kendrick Lamar - Swimming Pools (Drank)\r\nC:\\phone\\pools.mp3\r\n";
        std::fs::write(dir.join("a.m3u8"), body).unwrap();
        std::fs::write(dir.join("b.m3u8"), body).unwrap();
        let a = import_into_db(&mut conn, &parse_file(&dir.join("a.m3u8")).unwrap(), &dir).unwrap();
        let b = import_into_db(&mut conn, &parse_file(&dir.join("b.m3u8")).unwrap(), &dir).unwrap();

        let rv = playlist_review(&mut conn, a.id).unwrap();
        assert_eq!((rv.pending.len(), rv.matched, rv.decided.len()), (1, 1, 0), "{rv:#?}");
        let item = &rv.pending[0];
        assert_eq!(item.positions, vec![1, 3], "one decision covers both occurrences");
        assert_eq!(item.result.reason, Reason::VersionMismatch);
        assert_eq!(item.result.candidates.len(), 2, "both versions offered, ranked");
        let key = item.result.ref_key.clone();

        // Pick a candidate: both playlists follow, the item leaves the queue.
        let chosen = inst.to_string_lossy().to_string();
        assert_eq!(decide(&mut conn, &key, "accept", Some(&chosen)).unwrap(), 2);
        for id in [a.id, b.id] {
            let items = db::playlist_items(&conn, id).unwrap();
            assert_eq!((items[0].0.as_str(), items[2].0.as_str()), (chosen.as_str(), chosen.as_str()));
        }
        let rv = playlist_review(&mut conn, a.id).unwrap();
        assert_eq!((rv.pending.len(), rv.decided.len()), (0, 1));
        assert!(decide(&mut conn, &key, "accept", Some(r"C:\nope.flac")).is_err(), "can't accept a file that doesn't exist");

        // Undo: back to the original location and back in the queue.
        decide(&mut conn, &key, "clear", None).unwrap();
        assert_eq!(db::playlist_items(&conn, b.id).unwrap()[0].0, r"C:\phone\pools.mp3");
        assert_eq!(playlist_review(&mut conn, b.id).unwrap().pending.len(), 1);

        // Not in my library: never offered again, listed under decided.
        decide(&mut conn, &key, "missing", None).unwrap();
        let rv = playlist_review(&mut conn, b.id).unwrap();
        assert_eq!((rv.pending.len(), rv.decided.len(), rv.decided[0].result.reason), (0, 1, Reason::MarkedMissing));
        assert!(decide(&mut conn, &key, "maybe", None).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Parse every playlist under `fixtures/` (the repo root) with every parser that claims it.
    /// The real samples aren't there yet; this reports how many were checked so a pass with zero
    /// files is visible as such.
    #[test]
    fn real_fixture_files_parse() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("fixtures");
        let mut checked = 0;
        if root.is_dir() {
            for e in walkdir::WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
                let ext = e.path().extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase();
                if e.file_type().is_file() && PLAYLIST_EXTENSIONS.contains(&ext.as_str()) {
                    let pl = parse_file(e.path()).unwrap_or_else(|err| panic!("{}: {err:#}", e.path().display()));
                    assert!(!pl.items.is_empty(), "{} parsed to no items", e.path().display());
                    checked += 1;
                }
            }
        }
        eprintln!("real_fixture_files_parse: {checked} fixture playlist file(s) checked under {}", root.display());
    }

    #[test]
    fn decodes_boms_and_legacy_encodings() {
        assert_eq!(decode_text(b"\xEF\xBB\xBFa\xC3\xA9"), "aé");
        let utf16le: Vec<u8> = [0xFF, 0xFE].into_iter().chain("Мало".encode_utf16().flat_map(|u| u.to_le_bytes())).collect();
        assert_eq!(decode_text(&utf16le), "Мало");
        let utf16be: Vec<u8> = [0xFE, 0xFF].into_iter().chain("x€".encode_utf16().flat_map(|u| u.to_be_bytes())).collect();
        assert_eq!(decode_text(&utf16be), "x€");
        // Windows-1252, not Latin-1: 0x92 is a curly apostrophe, 0x80 the euro sign.
        assert_eq!(decode_text(b"Don\x92t \x80 caf\xE9"), "Don’t € café");
    }

    #[test]
    fn file_uris() {
        assert_eq!(file_uri_to_path("file:///C:/a%20b/c.flac").unwrap(), r"C:\a b\c.flac");
        assert_eq!(file_uri_to_path("file://localhost/C:/x.mp3").unwrap(), r"C:\x.mp3");
        assert_eq!(file_uri_to_path("FILE:///c|/x.mp3").unwrap(), r"c:\x.mp3");
        assert_eq!(file_uri_to_path("file:/C:/x.mp3").unwrap(), r"C:\x.mp3");
        assert_eq!(file_uri_to_path("file://nas/share/M%C3%BAsica/x.mp3").unwrap(), r"\\nas\share\Música\x.mp3");
        assert_eq!(file_uri_to_path("file:///storage/emulated/0/Music/x.mp3").unwrap(), r"\storage\emulated\0\Music\x.mp3");
        assert!(file_uri_to_path("http://x").is_none());
    }

    #[test]
    fn uris_round_trip() {
        for loc in [r"C:\a b\c#1 [x] 100%.flac", r"\\nas\share\Мало тебя.flac", r"sub\dir\é.mp3", r"\storage\emulated\0\x.mp3"] {
            let uri = location_to_uri(loc);
            assert!(uri.is_ascii(), "{uri}");
            assert_eq!(uri_to_location(&uri).unwrap(), loc, "{uri}");
        }
        assert_eq!(location_to_uri(r"C:\a b\c.flac"), "file:///C:/a%20b/c.flac");
        assert_eq!(location_to_uri(r"\\nas\share\x.flac"), "file://nas/share/x.flac");
        assert_eq!(location_to_uri("http://radio/x"), "http://radio/x");
    }

    #[test]
    fn urls_vs_drive_letters() {
        assert!(is_url("http://x/y"));
        assert!(is_url("spotify:track:abc"));
        assert!(!is_url(r"C:\x.mp3"));
        assert!(!is_url("C:/x.mp3"));
        assert!(!is_url("relative/a:b.mp3"));
    }

    #[test]
    fn resolves_relative_rooted_and_percent_paths() {
        let dir = std::env::temp_dir().join(format!("ic-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let base = dir.as_path();
        assert_eq!(resolve_location(r"..\x.mp3", base).unwrap(), dir.parent().unwrap().join("x.mp3").to_string_lossy());
        assert_eq!(resolve_location(r"\storage\emulated\0\x.mp3", base).unwrap(), r"\storage\emulated\0\x.mp3");
        assert!(resolve_location("http://radio", base).is_none());
        // Encoded entry for a file that doesn't exist: decoded.
        assert!(resolve_location("01%20Song.mp3", base).unwrap().ends_with("01 Song.mp3"));
        // A real file whose name contains %AB is not mangled.
        std::fs::write(dir.join("100%AB.mp3"), b"").unwrap();
        assert!(resolve_location("100%AB.mp3", base).unwrap().ends_with("100%AB.mp3"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn identifiers() {
        assert_eq!(normalize_isrc("us-um7-16-01827").as_deref(), Some("USUM71601827"));
        assert_eq!(normalize_isrc("50999 9 09 790 2 6"), None);
        assert_eq!(normalize_isrc(""), None);
        assert_eq!(service_id_from_uri("https://open.spotify.com/track/abc?si=1"), Some(("spotify", "abc".into())));
        assert_eq!(service_id_from_uri("https://music.youtube.com/watch?list=x&v=dQw4w9WgXcQ"), Some(("youtube", "dQw4w9WgXcQ".into())));
        assert_eq!(mbid_from_uri("https://musicbrainz.org/recording/6C4E3B79-5345-4854-AC55-82AD1EC2D928").as_deref(), Some("6c4e3b79-5345-4854-ac55-82ad1ec2d928"));
    }
}
