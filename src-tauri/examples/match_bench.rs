//! Matcher speed on a generated 50k-track library:
//! `cargo run --release --example match_bench [-- <tracks> <refs>]`
//!
//! The library is synthetic but built to be hard: a small vocabulary (so many titles share
//! words), multi-artist credits, ~8% of songs with sibling versions (remix / extended / live /
//! slowed…), ~2% duplicated in a second folder, ISRCs on 40%. References with a known answer are
//! generated from it: positives with the formatting noise real sources add, version siblings that
//! aren't in the library, same-title songs by other artists, and absent songs.
//!
//! It reports index build time and per-reference latency. The accuracy numbers are a synthetic
//! sanity check of the rules, **not** the golden-set evaluation (that needs real playlists).

use std::collections::HashSet;
use std::time::Instant;

use resonance_lib::library::db;
use resonance_lib::library::interchange::TrackRef;
use resonance_lib::library::matcher::{Matcher, Policy, Reason};
use resonance_lib::library::model::Track;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn chance(&mut self, p: f64) -> bool {
        (self.next() % 1_000_000) as f64 / 1e6 < p
    }
    fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len())]
    }
}

const WORDS: &[&str] = &[
    "love", "night", "heart", "fire", "dream", "light", "dance", "summer", "rain", "home", "gold", "wild", "blue", "falling",
    "forever", "baby", "time", "city", "lost", "girl", "boy", "run", "away", "stars", "sky", "ocean", "shadow", "midnight",
    "electric", "sugar", "honey", "storm", "paradise", "faded", "alive", "young", "free", "tonight", "money", "crazy",
    "dark", "angel", "devil", "ghost", "river", "highway", "neon", "velvet", "silver", "echo", "kiss", "tears", "smile",
    "secret", "magic", "thunder", "lightning", "hello", "goodbye", "sweet", "bitter", "cold", "warm", "slow", "fast",
    "high", "low", "moon", "sun", "world", "dancer", "lover", "hunter", "golden", "broken", "perfect", "beautiful",
    "everything", "nothing", "somebody", "nobody", "again", "tomorrow", "yesterday", "back", "down", "up", "together",
    "alone", "tokyo", "paris", "berlin", "miami", "мало", "тебя", "corazón", "noche", "amor", "fuego", "sueño", "日本",
    "étoile", "über", "fly", "drive", "burn", "shine", "feel", "need", "want", "know", "tell", "call", "wait", "stay",
];
const SYLLABLES: &[&str] = &[
    "ka", "ri", "mo", "zen", "lo", "va", "nix", "tor", "el", "sa", "ky", "ro", "lux", "dra", "mi", "jo", "ne", "ta", "vo",
    "ash", "qui", "ber", "lin", "ox", "ny", "fa", "gu", "ze", "pha", "ro",
];
const VERSIONS: &[&str] = &[
    "(Remix)", "[Extended]", "(Live)", "(Acoustic)", "(Slowed + Reverb)", "(Sped Up)", "(Instrumental)", "(Radio Edit)",
    "(Club Mix)", "(Nightcore)", "(VIP Mix)", "(Piano Version)",
];
const NOISE: &[&str] = &["(Official Video)", "(Official Audio)", "[Lyrics]", "(Visualizer)", "[HD]"];

fn name(rng: &mut Rng) -> String {
    let n = 2 + rng.below(2);
    let mut s: String = (0..n).map(|_| *rng.pick(SYLLABLES)).collect();
    s[..1].make_ascii_uppercase();
    if rng.chance(0.15) {
        s = format!("{s} {}", ["Collective", "Band", "Crew", "Project", "Sound"][rng.below(5)]);
    }
    if rng.chance(0.1) {
        s = format!("DJ {s}");
    }
    s
}

fn title(rng: &mut Rng) -> String {
    let n = 1 + rng.below(4);
    let words: Vec<String> = (0..n)
        .map(|_| {
            let w = *rng.pick(WORDS);
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect();
    words.join(" ")
}

fn isrc(rng: &mut Rng, n: usize) -> String {
    format!("{}{}{:02}{:05}", ["US", "GB", "DE", "QZ", "FR"][rng.below(5)], ["UM7", "AT2", "KPL", "HR1", "XYZ"][rng.below(5)], rng.below(30), n % 100_000)
}

struct Song {
    title: String,
    artist: String,
    secs: f64,
    isrc: Option<String>,
    path: String,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let n_tracks: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(50_000);
    let n_refs: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(5_000);
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let artists: Vec<String> = (0..4_000).map(|_| name(&mut rng)).collect();

    // ---- library -------------------------------------------------------------------------------
    let mut songs: Vec<Song> = Vec::with_capacity(n_tracks);
    let mut seen: HashSet<(String, String)> = HashSet::new();
    while songs.len() < n_tracks {
        let mut artist = rng.pick(&artists).clone();
        if rng.chance(0.12) {
            artist = format!("{artist} {} {}", ["feat.", "&", "x", ","][rng.below(4)], rng.pick(&artists));
        }
        let t = title(&mut rng);
        if !seen.insert((t.to_lowercase(), artist.to_lowercase())) {
            continue;
        }
        let secs = 90.0 + rng.below(330) as f64 + rng.below(10) as f64 / 10.0;
        let n = songs.len();
        let isrc = rng.chance(0.4).then(|| isrc(&mut rng, n));
        let path = format!(r"D:\Synth\{artist}\Album {}\{:02} {t}.flac", rng.below(40), rng.below(20) + 1);
        songs.push(Song { title: t.clone(), artist: artist.clone(), secs, isrc, path });
        // Sibling versions of the same song: the confusers that matter.
        if rng.chance(0.08) && songs.len() < n_tracks {
            let v = *rng.pick(VERSIONS);
            let secs2 = if v.contains("Extended") { secs + 90.0 } else { secs + rng.below(20) as f64 - 10.0 };
            let path = format!(r"D:\Synth\{artist}\Versions\{t} {v}.flac");
            songs.push(Song { title: format!("{t} {v}"), artist: artist.clone(), secs: secs2, isrc: None, path });
        }
        // The same file twice (a copy in another folder): must come out ambiguous, never a coin flip.
        if rng.chance(0.02) && songs.len() < n_tracks {
            let path = format!(r"D:\Synth\Copies\{artist} - {t}.flac");
            songs.push(Song { title: t.clone(), artist: artist.clone(), secs, isrc: None, path });
        }
    }

    let dir = std::env::temp_dir().join(format!("resonance-match-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut conn = db::open(&dir.join("library.db")).unwrap();
    let folder = db::add_folder(&conn, r"D:\Synth").unwrap();
    let t0 = Instant::now();
    let rows: Vec<(Track, String)> = songs
        .iter()
        .map(|s| {
            (
                Track { path: s.path.clone(), title: s.title.clone(), artist: s.artist.clone(), duration: s.secs, mtime: 1, isrc: s.isrc.clone(), ..Default::default() },
                r"D:\Synth".to_string(),
            )
        })
        .collect();
    for chunk in rows.chunks(5_000) {
        db::upsert_tracks(&mut conn, folder, chunk).unwrap();
    }
    println!("library: {} tracks generated and stored in {:.2} s", songs.len(), t0.elapsed().as_secs_f64());

    let t0 = Instant::now();
    let matcher = Matcher::open(&mut conn).unwrap();
    let cold = t0.elapsed();
    let t0 = Instant::now();
    let reindexed = resonance_lib::library::matcher::sync_fts(&mut conn).unwrap();
    let sync = t0.elapsed();
    let t0 = Instant::now();
    let tracks = db::all_tracks(&conn).unwrap();
    let load = t0.elapsed();
    drop(tracks);
    let t0 = Instant::now();
    let matcher2 = Matcher::open(&mut conn).unwrap();
    let warm = t0.elapsed();
    drop(matcher2);
    println!("index:   first open {:.2} s (FTS5 build for {} tracks + load), next open {:.2} s (nothing to re-index)", cold.as_secs_f64(), matcher.len(), warm.as_secs_f64());
    println!("         of which: FTS sync check {:.0} ms ({reindexed} re-indexed), loading tracks {:.0} ms, readings cache + maps the rest", sync.as_secs_f64() * 1000.0, load.as_secs_f64() * 1000.0);

    // ---- references ----------------------------------------------------------------------------
    // Ground truth: the files that are a right answer (several for a song stored twice: both are
    // the same recording); empty = not in the library.
    let titles_by_song: HashSet<(String, String)> = songs.iter().map(|s| (s.title.to_lowercase(), s.artist.to_lowercase())).collect();
    let copies: std::collections::HashMap<(String, String), Vec<String>> = {
        let mut m: std::collections::HashMap<(String, String), Vec<String>> = std::collections::HashMap::new();
        for s in &songs {
            m.entry((s.title.to_lowercase(), s.artist.to_lowercase())).or_default().push(s.path.clone());
        }
        m
    };
    let mut refs: Vec<(&'static str, TrackRef, Vec<String>)> = Vec::with_capacity(n_refs);
    while refs.len() < n_refs {
        let s = &songs[rng.below(songs.len())];
        let ms = |secs: f64, rng: &mut Rng| Some(((secs + (rng.below(50) as f64 - 25.0) / 10.0) * 1000.0) as u64);
        match refs.len() * 10 / n_refs {
            // 40%: positives, with the noise real sources add.
            0..=3 => {
                let mut t = TrackRef { title: s.title.clone(), artists: vec![s.artist.clone()], duration_ms: ms(s.secs, &mut rng), ..Default::default() };
                match rng.below(6) {
                    0 => t.title = format!("{} {}", t.title, rng.pick(NOISE)),
                    1 => t.artists = vec![format!("{} - Topic", s.artist)],
                    2 => {
                        t.title = format!("{} - {}", s.artist, s.title);
                        t.artists.clear();
                    }
                    3 => t.title = t.title.to_uppercase(),
                    4 => t.isrc = s.isrc.clone(),
                    _ => {}
                }
                let paths = copies[&(s.title.to_lowercase(), s.artist.to_lowercase())].clone();
                refs.push((if paths.len() > 1 { "song stored twice" } else { "positive" }, t, paths));
            }
            // 30%: a version of a library song that the library doesn't have.
            4..=6 => {
                let v = *rng.pick(VERSIONS);
                let vt = format!("{} {v}", s.title);
                if titles_by_song.contains(&(vt.to_lowercase(), s.artist.to_lowercase())) || s.title.contains('(') || s.title.contains('[') {
                    continue;
                }
                let t = TrackRef { title: vt, artists: vec![s.artist.clone()], duration_ms: ms(s.secs + 30.0, &mut rng), ..Default::default() };
                refs.push(("version not in library", t, Vec::new()));
            }
            // 15%: the same title by an artist who doesn't have it.
            7 => {
                let other = rng.pick(&artists).clone();
                if titles_by_song.contains(&(s.title.to_lowercase(), other.to_lowercase())) {
                    continue;
                }
                let t = TrackRef { title: s.title.clone(), artists: vec![other], duration_ms: ms(s.secs, &mut rng), ..Default::default() };
                refs.push(("same title, other artist", t, Vec::new()));
            }
            // 15%: absent songs.
            _ => {
                let t = TrackRef { title: format!("{} {}", title(&mut rng), title(&mut rng)), artists: vec![name(&mut rng)], duration_ms: Some(200_000), ..Default::default() };
                refs.push(("absent", t, Vec::new()));
            }
        }
    }

    // ---- match ---------------------------------------------------------------------------------
    let policy = Policy { fuzzy_auto: true };
    let mut lat: Vec<f64> = Vec::with_capacity(refs.len());
    let t_all = Instant::now();
    let mut results = Vec::with_capacity(refs.len());
    for (_, r, _) in &refs {
        let t0 = Instant::now();
        let res = matcher.match_ref(&conn, r, None, policy).unwrap();
        lat.push(t0.elapsed().as_secs_f64() * 1000.0);
        results.push(res);
    }
    let total = t_all.elapsed().as_secs_f64();
    let lat_by_ref = lat.clone();
    lat.sort_by(f64::total_cmp);
    let q = |p: f64| lat[((lat.len() as f64 - 1.0) * p) as usize];
    println!(
        "match:   {} refs in {:.2} s = {:.0} refs/s; per ref p50 {:.2} ms, p95 {:.2} ms, p99 {:.2} ms, max {:.2} ms",
        refs.len(),
        total,
        refs.len() as f64 / total,
        q(0.5),
        q(0.95),
        q(0.99),
        lat[lat.len() - 1]
    );

    // ---- synthetic sanity check ----------------------------------------------------------------
    println!("\nSynthetic sanity check (generated data; NOT the golden-set evaluation):");
    println!("{:<26} {:>6} {:>10} {:>8} {:>8} {:>8}", "kind", "refs", "auto ok", "WRONG", "review", "none");
    let kinds = ["positive", "song stored twice", "version not in library", "same title, other artist", "absent"];
    let (mut wrong_total, mut auto_total) = (0, 0);
    for kind in kinds {
        let (mut n, mut ok, mut wrong, mut review, mut none) = (0, 0, 0, 0, 0);
        for ((k, _, truth), res) in refs.iter().zip(&results) {
            if *k != kind {
                continue;
            }
            n += 1;
            match (&res.accepted, truth) {
                (Some(c), t) if t.contains(&c.path) => ok += 1,
                (Some(_), _) => wrong += 1,
                (None, _) if res.needs_review() => review += 1,
                (None, _) => none += 1,
            }
        }
        auto_total += ok + wrong;
        wrong_total += wrong;
        let mut l: Vec<f64> = refs.iter().zip(&lat_by_ref).filter(|((k, _, _), _)| *k == kind).map(|(_, ms)| *ms).collect();
        l.sort_by(f64::total_cmp);
        let p50 = l.get(l.len() / 2).copied().unwrap_or(0.0);
        println!("{kind:<26} {n:>6} {ok:>10} {wrong:>8} {review:>8} {none:>8}   p50 {p50:.2} ms");
    }
    let reasons = |want: Reason| results.iter().filter(|r| r.reason == want).count();
    println!(
        "auto-accepted {auto_total}, wrong {wrong_total} ({:.2}% precision on synthetic data); reasons: ambiguous {}, version mismatch {}, low confidence {}, duration {}, no candidate {}",
        if auto_total > 0 { 100.0 * (auto_total - wrong_total) as f64 / auto_total as f64 } else { 0.0 },
        reasons(Reason::Ambiguous),
        reasons(Reason::VersionMismatch),
        reasons(Reason::LowConfidence),
        reasons(Reason::DurationMismatch),
        reasons(Reason::NoCandidate)
    );
    for ((k, r, truth), res) in refs.iter().zip(&results).filter(|((_, _, t), res)| res.accepted.as_ref().is_some_and(|c| !t.contains(&c.path))).take(5) {
        println!("  WRONG [{k}] {:?} by {:?} -> {} ({}) truth {:?}", r.title, r.artists, res.accepted.as_ref().unwrap().path, res.explanation, truth);
    }
    for ((_, r, _), res) in refs.iter().zip(&results).filter(|((k, _, _), res)| *k == "positive" && res.accepted.is_none()).take(6) {
        println!("  missed positive: {:?} by {:?} ({:?} ms): {} {}", r.title, r.artists, r.duration_ms, res.reason.name(), res.explanation);
    }
    drop(matcher);
    drop(conn);
    let _ = std::fs::remove_dir_all(&dir);
}
