//! Text normalisation for matching track references to local files.
//!
//! The rules are symmetric: the same function runs on both sides of every comparison. That way a
//! misclassified bracket (say a title that really is "(Live Your Life)") makes two sources that
//! format it differently fail to match, a miss, rather than making two different recordings
//! look identical, a wrong match.
//!
//! What the rules separate:
//! - **packaging noise** (removed): "(Official Video)", "[Lyrics]", "(Audio)", "(prod. by …)",
//!   YouTube ids, " - Topic" / "VEVO" channel names;
//! - **featured artists** (moved into the artist set): "(feat. X)", "ft. X", "(with X)";
//! - **version qualifiers** (kept and compared exactly): remix, edit, mix, slowed, sped up,
//!   reverb, instrumental, extended, live, acoustic, hardstyle, "best part", V3, …; a
//!   different set means a different recording;
//! - **drift** (kept; never auto-accepted, fine to pick in review): remaster, explicit/clean,
//!   album/single version, original mix.

use std::collections::BTreeSet;

use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

/// Bump when any rule here changes; the FTS candidate table is rebuilt from it.
pub const NORM_VERSION: i64 = 2;

/// NFKC compatibility mapping (𝘼 → A, full-width → ASCII, ligatures), accents removed, case
/// folded, typographic punctuation unified, `$` read as `s` (Ke$ha, WOR$T, A$AP), and invisible
/// characters, symbols and emoji turned into spaces. Brackets, dashes and list separators are
/// kept because the parsers below need them.
pub fn fold(s: &str) -> String {
    // Most titles are ASCII, where NFKD is the identity: skip the decomposition (about 10× faster
    // over a 50k-track library). `fold_unicode` gives the same result; a test checks it.
    if s.is_ascii() {
        fold_ascii(s)
    } else {
        fold_unicode(s)
    }
}

fn push_folded(c: char, out: &mut String) {
    if c.is_alphanumeric() {
        out.extend(c.to_lowercase());
    } else if "-()[]{}&,;/+|'.:!?\"#*_~@".contains(c) {
        out.push(c);
    } else {
        out.push(' '); // whitespace, braille blank U+2800, symbols, emoji, control/format chars
    }
}

fn fold_ascii(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        let c = match b {
            b'`' => '\'',
            b'$' => 's',
            _ => b as char,
        };
        push_folded(c, &mut out);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn fold_unicode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    // NFKD = NFKC's compatibility mappings, but with accents split off so they can be dropped.
    for c in s.nfkd() {
        if is_combining_mark(c) {
            continue;
        }
        let c = match c {
            '\u{2019}' | '\u{2018}' | '`' | '\u{b4}' | '\u{2bc}' | '\u{2032}' => '\'',
            '\u{201c}' | '\u{201d}' | '\u{201e}' => '"',
            '\u{2010}'..='\u{2015}' | '\u{2212}' => '-',
            '$' => 's',
            _ => c,
        };
        push_folded(c, &mut out);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Letters and digits only: "E.T." → "et", "GEN.KLOUD" → "genkloud", "Dance..." → "dance".
pub fn key(s: &str) -> String {
    fold(s).chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Word tokens: apostrophes and dots join ("don't" → "dont", "e.t." → "et"), `&` reads as "and".
pub fn tokens(s: &str) -> Vec<String> {
    let f = fold(s).replace(['\'', '.'], "").replace('&', " and ");
    f.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()).map(str::to_string).collect()
}

const NOISE: &[&str] = &[
    "official", "oficial", "video", "videoclip", "clip", "audio", "music", "musicvideo", "mv", "lyric", "lyrics",
    "visualizer", "visualiser", "hd", "hq", "4k", "1080p", "720p", "with", "full",
];
const VERSION_WORDS: &[&str] = &[
    "remix", "mix", "edit", "version", "slowed", "reverb", "sped", "nightcore", "instrumental", "acapella", "karaoke",
    "extended", "live", "acoustic", "unplugged", "demo", "cover", "hardstyle", "hardtekk", "phonk", "boosted", "8d",
    "loop", "mashup", "bootleg", "vip", "rework", "flip", "piano", "orchestral", "remake", "chopped", "screwed",
    "tiktok", "nightcore", "remixed",
];
const DRIFT_WORDS: &[&str] =
    &["remaster", "remastered", "remasterizado", "remasterise", "explicit", "clean", "dirty", "censored", "uncensored", "mono", "stereo", "deluxe", "bonus"];
const DRIFT_PHRASES: &[&[&str]] =
    &[&["album", "version"], &["single", "version"], &["original", "mix"], &["original", "version"], &["radio", "version"]];
const FILLERS: &[&str] = &["version", "ver", "the", "a", "and", "up"];
/// Words that are a qualifier even alone after " - " ("Song - Remix"). Others ("TikTok", "Cover",
/// "Loop", "Piano") are as likely to be the title itself.
const STRONG_SINGLE: &[&str] = &[
    "remix", "edit", "slowed", "reverb", "sped", "nightcore", "instrumental", "acapella", "karaoke", "extended", "live",
    "acoustic", "unplugged", "remaster", "explicit", "clean", "mono", "stereo", "hardstyle", "hardtekk", "phonk",
];

fn canonical_word(t: &str) -> &str {
    match t {
        "rmx" => "remix",
        "reverbed" => "reverb",
        "speed" | "spedup" => "sped",
        "inst" => "instrumental",
        "acappella" | "cappella" => "acapella",
        "remastered" => "remaster",
        _ => t,
    }
}

fn is_version_number(t: &str) -> bool {
    t.len() >= 2 && t.starts_with('v') && t[1..].chars().all(|c| c.is_ascii_digit())
}

/// YouTube video ids appear in brackets in downloaded file names: 11 chars of [A-Za-z0-9_-] with
/// at least one digit or mixed case.
fn is_youtube_id(raw: &str) -> bool {
    let s = raw.trim();
    s.len() == 11
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && (s.chars().any(|c| c.is_ascii_digit() || c == '_') || (s.chars().any(|c| c.is_ascii_uppercase()) && s.chars().any(|c| c.is_ascii_lowercase())))
}

#[derive(Debug, PartialEq)]
enum Segment {
    Noise,
    Featured(Vec<String>),
    Version(String),
    Drift(String),
    Keep,
}

/// Sorted, de-duplicated canonical words, fillers dropped: "Slowed + Reverbed" and
/// "slowed & reverb" both become "reverb slowed".
fn canonical(toks: &[String]) -> String {
    let set: BTreeSet<&str> = toks.iter().map(|t| canonical_word(t)).filter(|t| !FILLERS.contains(t)).collect();
    set.into_iter().collect::<Vec<_>>().join(" ")
}

fn classify(raw: &str) -> Segment {
    if is_youtube_id(raw) {
        return Segment::Noise;
    }
    let toks = tokens(raw);
    let Some(first) = toks.first() else { return Segment::Noise };
    if toks.iter().all(|t| NOISE.contains(&t.as_str())) {
        return Segment::Noise;
    }
    if first == "prod" || (first == "produced" && toks.get(1).is_some_and(|t| t == "by")) {
        return Segment::Noise;
    }
    if ["feat", "ft", "featuring", "with"].contains(&first.as_str()) && toks.len() > 1 {
        let f = fold(raw);
        let rest = f.split_once(' ').map(|(_, r)| r).unwrap_or("");
        return Segment::Featured(split_artists(rest));
    }
    let drift = toks.iter().any(|t| DRIFT_WORDS.contains(&t.as_str()))
        || DRIFT_PHRASES.iter().any(|p| toks.windows(p.len()).any(|w| w.iter().zip(p.iter()).all(|(a, b)| a == b)));
    if drift {
        return Segment::Drift(canonical(&toks));
    }
    let version = toks.iter().any(|t| VERSION_WORDS.contains(&canonical_word(t)) || is_version_number(t))
        || toks.windows(2).any(|w| w[0] == "best" && w[1] == "part")
        || toks.windows(2).any(|w| (w[0] == "sped" || w[0] == "speed") && w[1] == "up");
    if version {
        return Segment::Version(canonical(&toks));
    }
    Segment::Keep
}

/// A title taken apart.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ParsedTitle {
    /// What's left: folded, still with " - " if the title had one.
    pub main: String,
    pub featured: Vec<String>,
    pub versions: BTreeSet<String>,
    pub drift: BTreeSet<String>,
}

fn take_segment(seg: Segment, p: &mut ParsedTitle) -> bool {
    match seg {
        Segment::Keep => return false,
        Segment::Noise => {}
        Segment::Featured(a) => p.featured.extend(a),
        Segment::Version(v) => {
            p.versions.insert(v);
        }
        Segment::Drift(d) => {
            p.drift.insert(d);
        }
    }
    true
}

pub fn parse_title(title: &str) -> ParsedTitle {
    let f = fold(title);
    let mut p = ParsedTitle::default();

    // 1. Bracketed segments ( ), [ ], { } at the top level. Ones that aren't noise, featuring or
    //    a qualifier stay in the title: "Swimming Pools (Drank)", "sins (let me in)".
    let mut main = String::with_capacity(f.len());
    let mut depth = 0usize;
    let mut seg = String::new();
    for c in f.chars() {
        match c {
            '(' | '[' | '{' => {
                if depth > 0 {
                    seg.push(c);
                }
                depth += 1;
            }
            ')' | ']' | '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    let s = std::mem::take(&mut seg);
                    if !take_segment(classify(&s), &mut p) {
                        main.push('(');
                        main.push_str(&s);
                        main.push(')');
                    }
                    main.push(' ');
                } else {
                    seg.push(c);
                }
            }
            _ if depth > 0 => seg.push(c),
            _ => main.push(c),
        }
    }
    if depth > 0 {
        main.push_str(&seg); // unbalanced: keep the text
    }
    let mut main = main.split_whitespace().collect::<Vec<_>>().join(" ");

    // 2. Trailing " - <qualifier>" parts: "Swimming Pools (Drank) - Extended Version",
    //    "Song - 2011 Remaster", "Song - Live at Wembley". A single word after the dash only counts
    //    when it can't be a title: "Kesha - TikTok" is a song called TikTok, not a TikTok version.
    while let Some(i) = main.rfind(" - ") {
        let suffix = &main[i + 3..];
        let toks = tokens(suffix);
        let seg = classify(suffix);
        let ambiguous = toks.len() == 1
            && !matches!(seg, Segment::Noise)
            && !STRONG_SINGLE.contains(&canonical_word(&toks[0]));
        if ambiguous || !take_segment(seg, &mut p) {
            break;
        }
        main.truncate(i);
    }

    // 3. Unbracketed featuring: "Song feat. X". In "A feat. B - Song" the credit belongs to the
    //    artist part, which the "Artist - Title" reading splits; only the title side is cut here.
    let title_start = main.find(" - ").map(|i| i + 3).unwrap_or(0);
    for sep in [" feat. ", " feat ", " ft. ", " ft ", " featuring "] {
        if let Some(i) = main[title_start..].find(sep).map(|i| i + title_start) {
            p.featured.extend(split_artists(&main[i + sep.len()..]));
            main.truncate(i);
        }
    }
    p.main = main.trim().trim_end_matches(['-', ' ']).to_string();
    p
}

/// Artist string → artist keys. Splits on featuring, `&`, `and`, `x`, `,`, `;`, `/`, `+`, `vs`,
/// `with`, and drops " - Topic" and "VEVO" channel suffixes. "vivi baby; ovg!" and
/// "vivi baby & ovg!" both give ["vivibaby", "ovg"].
pub fn split_artists(s: &str) -> Vec<String> {
    let mut f = fold(s);
    for suffix in [" - topic", " topic"] {
        if let Some(stripped) = f.strip_suffix(suffix) {
            f = stripped.to_string();
        }
    }
    let mut f = format!(" {f} ");
    for sep in [
        " feat. ", " feat ", " ft. ", " ft ", " featuring ", " & ", " and ", " x ", ",", ";", " / ", "/", " + ", " vs. ", " vs ", " with ",
    ] {
        f = f.replace(sep, "\u{1}");
    }
    let mut out: Vec<String> = Vec::new();
    for part in f.split('\u{1}') {
        let mut k = key(part);
        if k.len() > 6 && k.ends_with("vevo") {
            k.truncate(k.len() - 4);
        }
        if !k.is_empty() && !out.contains(&k) {
            out.push(k);
        }
    }
    out
}

/// One way of reading a title + artist pair.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Reading {
    pub artists: Vec<String>,
    pub title_key: String,
    pub title_tokens: Vec<String>,
    pub versions: BTreeSet<String>,
    pub drift: BTreeSet<String>,
    /// The artist came from an "Artist - Title" title rather than the artist field.
    pub from_title: bool,
}

/// Readings of a track: as tagged, and, when the title has "A - B", as artist A and title B.
/// The second covers uploads tagged with the uploader as artist ("RiFix" / "Ke$ha - TiK ToK
/// [Hardstyle]") and M3U display strings with no separate artist.
pub fn readings(title: &str, artists: &[String]) -> Vec<Reading> {
    let p = parse_title(title);
    let mut tagged: Vec<String> = Vec::new();
    for a in artists.iter().flat_map(|a| split_artists(a)).chain(p.featured.iter().cloned()) {
        if !tagged.contains(&a) {
            tagged.push(a);
        }
    }
    let mut out = vec![Reading {
        artists: tagged,
        title_key: key(&p.main),
        title_tokens: tokens(&p.main),
        versions: p.versions.clone(),
        drift: p.drift.clone(),
        from_title: false,
    }];
    if let Some(i) = p.main.find(" - ") {
        let (left, right) = (p.main[..i].trim(), p.main[i + 3..].trim());
        if !left.is_empty() && !right.is_empty() {
            let inner = parse_title(right); // "A - B ft. C"
            let mut a2 = split_artists(left);
            for x in inner.featured.iter().chain(p.featured.iter()) {
                if !a2.contains(x) {
                    a2.push(x.clone());
                }
            }
            let mut versions = p.versions.clone();
            versions.extend(inner.versions);
            let mut drift = p.drift.clone();
            drift.extend(inner.drift);
            out.push(Reading {
                artists: a2,
                title_key: key(&inner.main),
                title_tokens: tokens(&inner.main),
                versions,
                drift,
                from_title: true,
            });
        }
    }
    out
}

/// Dice coefficient on token sets, 0..1.
pub fn token_similarity(a: &[String], b: &[String]) -> f64 {
    let sa: BTreeSet<&str> = a.iter().map(String::as_str).collect();
    let sb: BTreeSet<&str> = b.iter().map(String::as_str).collect();
    if sa.is_empty() || sb.is_empty() {
        return 0.0;
    }
    2.0 * sa.intersection(&sb).count() as f64 / (sa.len() + sb.len()) as f64
}

/// Artist-set agreement, or `None` when either side has no artist. 1.0 = same set; 0.95 = both
/// primary artists appear on the other side; 0.9 = one set contains the other; else Jaccard.
pub fn artist_similarity(a: &[String], b: &[String]) -> Option<f64> {
    if a.is_empty() || b.is_empty() {
        return None;
    }
    let sa: BTreeSet<&str> = a.iter().map(String::as_str).collect();
    let sb: BTreeSet<&str> = b.iter().map(String::as_str).collect();
    if sa == sb {
        return Some(1.0);
    }
    if sb.contains(a[0].as_str()) && sa.contains(b[0].as_str()) {
        return Some(0.95);
    }
    if sa.is_subset(&sb) || sb.is_subset(&sa) {
        return Some(0.9);
    }
    Some(sa.intersection(&sb).count() as f64 / sa.union(&sb).count() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn folding() {
        assert_eq!(fold("𝘼𝙨𝙝𝙬𝙞𝙣𝙜⠀𖣂"), "ashwing 𖣂", "math letters → ASCII, braille blank → space");
        assert_eq!(fold("Frozy - From the Islands (kompa pasio\u{301}n)"), "frozy - from the islands (kompa pasion)");
        assert_eq!(fold("TUESDAY ｜ BP"), "tuesday | bp");
        assert_eq!(fold("Ke$ha – TiK ToK"), "kesha - tik tok");
        assert_eq!(fold("Alximo’s"), "alximo's");
        assert_eq!(key("E.T."), "et");
        assert_eq!(key("Мало тебя"), "малотебя");
        assert_eq!(tokens("Don't Stop & Go..."), s(&["dont", "stop", "and", "go"]));
        // The ASCII fast path agrees with the full path on every printable ASCII character.
        let all: String = (0x20u8..0x7f).map(|b| b as char).collect();
        for t in [all.as_str(), "Ke$ha - TiK ToK [Hardstyle]", "A`B (feat. C) {x} | y #1 ~z", "  Weird   Spacing\t\n"] {
            assert_eq!(fold_ascii(t), fold_unicode(t), "{t:?}");
        }
    }

    #[test]
    fn artists() {
        assert_eq!(split_artists("vivi baby; ovg!"), split_artists("vivi baby & ovg!"));
        assert_eq!(split_artists("Usher feat. Pitbull"), s(&["usher", "pitbull"]));
        assert_eq!(split_artists("FloyyMenor & Cris MJ"), s(&["floyymenor", "crismj"]));
        assert_eq!(split_artists("Kesha - Topic"), s(&["kesha"]));
        assert_eq!(split_artists("KeshaVEVO"), s(&["kesha"]));
        assert_eq!(split_artists("STARSET"), split_artists("Starset"));
        assert_eq!(split_artists("GEN.KLOUD"), s(&["genkloud"]));
    }

    #[test]
    fn qualifiers_and_noise() {
        let p = parse_title("Swimming Pools (Drank) (instrumental)");
        assert_eq!((p.main.as_str(), p.versions.iter().cloned().collect::<Vec<_>>()), ("swimming pools (drank)", s(&["instrumental"])));
        let a = parse_title("Swimming Pools (Drank) [Extended]");
        let b = parse_title("Swimming Pools (Drank) - Extended Version");
        assert_eq!((a.main.clone(), a.versions.clone()), (b.main.clone(), b.versions.clone()), "both are the extended version");
        let orig = parse_title("Swimming Pools (Drank)");
        assert!(orig.versions.is_empty() && orig.main == a.main, "the original has no qualifier");

        let e = parse_title("sins (let me in) [DarillisBeats Edit]");
        assert_eq!(e.main, "sins (let me in)");
        assert_eq!(e.versions.iter().next().unwrap(), "darillisbeats edit");

        let n = parse_title("Into You (Official Video) [HD]");
        assert_eq!((n.main.as_str(), n.versions.len(), n.drift.len()), ("into you", 0, 0));
        let f = parse_title("Rain Over Me (Feat. Marc Anthony)");
        assert_eq!((f.main.as_str(), f.featured.clone()), ("rain over me", s(&["marcanthony"])));
        let f2 = parse_title("Señorita ft. Camila Cabello");
        assert_eq!((f2.main.as_str(), f2.featured.clone()), ("senorita", s(&["camilacabello"])));

        assert_eq!(parse_title("Nice 2 Know Ya (Slowed+Reverbed)").versions, parse_title("Nice 2 Know Ya (slowed & reverb)").versions);
        assert_ne!(parse_title("x (super slowed & reverb)").versions, parse_title("x (slowed + reverb)").versions);
        assert_eq!(parse_title("x (Sped Up)").versions, parse_title("x - speed up").versions);
        assert!(!parse_title("Pretty Boy V3 (Best Part)").versions.is_empty());
        assert!(parse_title("Song - 2011 Remaster").drift.contains("2011 remaster"));
        assert!(parse_title("Song [Explicit]").drift.contains("explicit"));
        assert!(parse_title("Song (prod. by Metro Boomin)").versions.is_empty());
        assert_eq!(parse_title("Paparazzi [g2_bOaVAXcQ]").main, "paparazzi", "YouTube id dropped");
        assert_eq!(parse_title("From the Islands (kompa pasión)").main, "from the islands (kompa pasion)");
        assert_eq!(parse_title("Kesha - TikTok").main, "kesha - tiktok", "a one-word title isn't a qualifier");
        assert!(parse_title("Song - Remix").versions.contains("remix"));
        assert!(parse_title("Song (TikTok Version)").versions.contains("tiktok"));
    }

    #[test]
    fn readings_cover_uploader_tags_and_display_strings() {
        let local = readings("Ke$ha - TiK ToK [Hardstyle]", &s(&["RiFix"]));
        assert_eq!(local.len(), 2);
        assert_eq!((local[1].artists.clone(), local[1].title_key.as_str()), (s(&["kesha"]), "tiktok"));
        assert!(local[1].versions.contains("hardstyle"));
        let credit = readings("Oxkaox feat. DJ Tornixpha - Nothing Devil", &[]);
        assert_eq!((credit[1].artists.clone(), credit[1].title_key.as_str()), (s(&["oxkaox", "djtornixpha"]), "nothingdevil"));
        let m3u = readings("Daft Punk - One More Time", &[]);
        assert_eq!((m3u[1].artists.clone(), m3u[1].title_key.as_str()), (s(&["daftpunk"]), "onemoretime"));
        assert_eq!(readings("Into You", &s(&["Ariana Grande"])).len(), 1);
    }

    #[test]
    fn similarities() {
        assert_eq!(artist_similarity(&s(&["usher", "pitbull"]), &s(&["pitbull", "usher"])), Some(1.0));
        assert_eq!(artist_similarity(&s(&["pitbull"]), &s(&["pitbull", "marcanthony"])), Some(0.95), "primary artists agree");
        assert_eq!(artist_similarity(&s(&["marcanthony"]), &s(&["pitbull", "marcanthony"])), Some(0.9), "subset only");
        assert_eq!(artist_similarity(&[], &s(&["x"])), None);
        assert!(artist_similarity(&s(&["harrystyles"]), &s(&["mqx"])).unwrap() < 0.1);
        assert_eq!(token_similarity(&s(&["into", "you"]), &s(&["into", "you"])), 1.0);
    }
}
