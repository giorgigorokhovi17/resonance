//! Matcher behaviour on a library that mirrors the real one's metadata (titles, artists,
//! durations, ISRCs and MBIDs copied from MusicG14), including its hard cases.

use std::path::Path;

use rusqlite::Connection;

use super::*;
use crate::library::interchange::TrackRef;

const LIB: &str = r"C:\Users\G\MusicG14";

/// (relative path, title, artist, seconds, isrc, mbid)
const TRACKS: &[(&str, &str, &str, f64, &str, &str)] = &[
    (r"MQX\01 - Falling.flac", "Falling", "MQX", 154.6, "QZFP52231004", ""),
    (r"02-11 - Ariana Grande - Into You.flac", "Into You", "Ariana Grande", 244.5, "USUM71601827", "d64c6f5a-f529-468c-9a23-6bcdb213d14f"),
    (r"03 - Swimming Pools (Drank) (instrumental).flac", "Swimming Pools (Drank) (instrumental)", "Kendrick Lamar", 247.0, "", "05054cb0-26ed-45cd-9334-0a5102c537f7"),
    (r"09 Swimming Pools (Drank) [Extended].flac", "Swimming Pools (Drank) [Extended]", "Kendrick Lamar", 313.8, "", ""),
    (r"Kanii - sins (let me in) - 01 - sins (let me in).flac", "sins (let me in)", "Kanii", 128.3, "USWB12305416", ""),
    (r"sins (let me in) [DarillisBeats Edit].flac", "sins (let me in) [DarillisBeats Edit]", "Kanii", 128.3, "", ""),
    (r"Ke$ha - TiK ToK [Hardstyle].opus", "Ke$ha - TiK ToK [Hardstyle]", "RiFix", 146.8, "", ""),
    (r"Lady Gaga - Paparazzi.opus", "Lady Gaga - Paparazzi (Alximo’s dubstep remix) {Sped Up}", "X_.M150", 178.1, "", ""),
    (r"vivi baby & ovg! - MY JEALOUSY.flac", "MY JEALOUSY", "vivi baby; ovg!", 150.8, "QT6G32578110", ""),
    (r"097 - Pitbull - Rain Over Me (Feat. Marc Anthony).flac", "Rain Over Me (Feat. Marc Anthony)", "Pitbull", 231.6, "", ""),
    (r"110 - Usher feat. Pitbull - DJ Got Us Fallin' in Love.flac", "DJ Got Us Fallin' in Love", "Usher feat. Pitbull", 223.2, "", ""),
    (r"Cha-Ching.ogg", "Cha-Ching", "", 1.5, "", ""),
    (r"Serebro - Мало тебя.flac", "Мало тебя", "Serebro", 225.5, "RUA1D1611718", ""),
    (r"L.P. Rhythm - Versatile.opus", "L.P. Rhythm - Versatile (tiktok version) (super   slowed & reverb)", "𝘼𝙨𝙝𝙬𝙞𝙣𝙜⠀𖣂", 158.8, "", ""),
    (r"06 Heartbeat.flac", "Heartbeat", "Childish Gambino", 269.8, "", ""),
    ("Frozy - From the Islands (kompa pasio\u{301}n).flac", "From the Islands (kompa pasio\u{301}n)", "Frozy", 168.9, "USUS12400282", ""),
];

fn path(rel: &str) -> String {
    format!(r"{LIB}\{rel}")
}

fn library(extra: &[(&str, &str, &str, f64, &str, &str)]) -> Connection {
    let mut conn = db::open(Path::new(":memory:")).unwrap();
    let folder = db::add_folder(&conn, LIB).unwrap();
    let rows: Vec<(Track, String)> = TRACKS
        .iter()
        .chain(extra)
        .map(|(rel, title, artist, secs, isrc, mbid)| {
            let t = Track {
                path: path(rel),
                title: title.to_string(),
                artist: artist.to_string(),
                duration: *secs,
                mtime: 1,
                isrc: (!isrc.is_empty()).then(|| isrc.to_string()),
                mbid: (!mbid.is_empty()).then(|| mbid.to_string()),
                ..Default::default()
            };
            (t, LIB.to_string())
        })
        .collect();
    db::upsert_tracks(&mut conn, folder, &rows).unwrap();
    conn
}

fn r(title: &str, artists: &[&str], ms: Option<u64>) -> TrackRef {
    TrackRef { title: title.into(), artists: artists.iter().map(|a| a.to_string()).collect(), duration_ms: ms, ..Default::default() }
}

fn run(conn: &mut Connection, t: &TrackRef) -> MatchResult {
    run_with(conn, t, Policy { fuzzy_auto: true })
}

fn run_with(conn: &mut Connection, t: &TrackRef, policy: Policy) -> MatchResult {
    let m = Matcher::open(conn).unwrap();
    m.match_ref(conn, t, None, policy).unwrap()
}

#[track_caller]
fn accepted(res: &MatchResult, tier: Tier, rel: &str) {
    let got = res.accepted.as_ref().map(|c| c.path.clone());
    assert_eq!((got, res.tier), (Some(path(rel)), Some(tier)), "{res:#?}");
    assert_eq!(res.reason, Reason::None);
}

#[track_caller]
fn rejected(res: &MatchResult, reason: Reason) {
    assert!(res.accepted.is_none(), "must not be accepted: {res:#?}");
    assert_eq!(res.reason, reason, "{res:#?}");
}

// ---- positives --------------------------------------------------------------------------------

#[test]
fn isrc_tier() {
    let mut c = library(&[]);
    let mut t = r("Into You", &["Ariana Grande"], Some(244_000));
    t.isrc = Some("us-um7-16-01827".into());
    accepted(&run(&mut c, &t), Tier::Isrc, r"02-11 - Ariana Grande - Into You.flac");
}

#[test]
fn mbid_tier() {
    let mut c = library(&[]);
    let mut t = r("Swimming Pools (Drank) (Instrumental)", &["Kendrick Lamar"], Some(247_000));
    t.mbid = Some("05054CB0-26ED-45CD-9334-0A5102C537F7".into());
    accepted(&run(&mut c, &t), Tier::Mbid, r"03 - Swimming Pools (Drank) (instrumental).flac");
}

#[test]
fn fuzzy_through_packaging_noise_and_channel_names() {
    let mut c = library(&[]);
    let res = run(&mut c, &r("Into You (Official Video)", &["Ariana Grande - Topic"], Some(245_000)));
    accepted(&res, Tier::Fuzzy, r"02-11 - Ariana Grande - Into You.flac");
    assert!(res.explanation.contains("title same") && res.explanation.contains("Δ 0.5 s"), "{}", res.explanation);
}

#[test]
fn fuzzy_with_artist_inside_the_title() {
    let mut c = library(&[]);
    accepted(&run(&mut c, &r("Usher - DJ Got Us Fallin' In Love ft. Pitbull", &[], Some(223_000))), Tier::Fuzzy, r"110 - Usher feat. Pitbull - DJ Got Us Fallin' in Love.flac");
    accepted(&run(&mut c, &r("Rain Over Me", &["Pitbull", "Marc Anthony"], Some(231_000))), Tier::Fuzzy, r"097 - Pitbull - Rain Over Me (Feat. Marc Anthony).flac");
    accepted(&run(&mut c, &r("MY JEALOUSY", &["vivi baby", "ovg!"], Some(151_000))), Tier::Fuzzy, r"vivi baby & ovg! - MY JEALOUSY.flac");
    accepted(&run(&mut c, &r("Мало тебя", &["SEREBRO"], Some(225_000))), Tier::Fuzzy, r"Serebro - Мало тебя.flac");
}

#[test]
fn uploader_tagged_files_match_their_real_artist_and_version() {
    let mut c = library(&[]);
    accepted(&run(&mut c, &r("TiK ToK (Hardstyle)", &["Ke$ha"], Some(147_000))), Tier::Fuzzy, r"Ke$ha - TiK ToK [Hardstyle].opus");
    accepted(&run(&mut c, &r("Ke$ha - TiK ToK [Hardstyle]", &["RiFix"], Some(146_000))), Tier::Fuzzy, r"Ke$ha - TiK ToK [Hardstyle].opus");
}

#[test]
fn extended_version_named_two_ways() {
    let mut c = library(&[]);
    accepted(&run(&mut c, &r("Swimming Pools (Drank) - Extended Version", &["Kendrick Lamar"], Some(313_000))), Tier::Fuzzy, r"09 Swimming Pools (Drank) [Extended].flac");
}

#[test]
fn identical_durations_resolved_by_qualifier() {
    // Both files are 128.3 s; only the qualifier tells them apart.
    let mut c = library(&[]);
    accepted(&run(&mut c, &r("sins (let me in)", &["Kanii"], Some(128_000))), Tier::Fuzzy, r"Kanii - sins (let me in) - 01 - sins (let me in).flac");
    accepted(&run(&mut c, &r("sins (let me in) (DarillisBeats Edit)", &["Kanii"], Some(128_000))), Tier::Fuzzy, r"sins (let me in) [DarillisBeats Edit].flac");
}

// ---- hard negatives: absent recordings must not be attached to a neighbour ---------------------

#[test]
fn the_original_is_not_the_instrumental_or_the_extended() {
    let mut c = library(&[]);
    let res = run(&mut c, &r("Swimming Pools (Drank)", &["Kendrick Lamar"], Some(221_000)));
    rejected(&res, Reason::VersionMismatch);
    assert!(res.needs_review(), "shown in review with the candidates, never auto");
}

#[test]
fn other_versions_are_rejected() {
    let mut c = library(&[]);
    rejected(&run(&mut c, &r("sins (let me in) - Sped Up", &["Kanii"], Some(110_000))), Reason::VersionMismatch);
    rejected(&run(&mut c, &r("TiK ToK", &["Kesha"], Some(200_000))), Reason::VersionMismatch);
    rejected(&run(&mut c, &r("Paparazzi", &["Lady Gaga"], Some(208_000))), Reason::VersionMismatch);
    rejected(&run(&mut c, &r("Versatile (slowed + reverb)", &["L.P. Rhythm"], Some(158_000))), Reason::VersionMismatch);
}

#[test]
fn same_title_other_artist_is_no_candidate() {
    let mut c = library(&[]);
    let res = run(&mut c, &r("Falling", &["Harry Styles"], Some(240_000)));
    rejected(&res, Reason::NoCandidate);
    assert!(!res.needs_review(), "doesn't clutter the review queue");
    rejected(&run(&mut c, &r("Heartbeat", &["The Fray"], Some(269_000))), Reason::NoCandidate);
}

#[test]
fn same_artist_other_song_is_no_candidate() {
    let mut c = library(&[]);
    let res = run(&mut c, &r("7 rings", &["Ariana Grande"], Some(178_000)));
    rejected(&res, Reason::NoCandidate);
    assert!(!res.needs_review(), "an artist in common isn't enough to ask: {res:#?}");
    rejected(&run(&mut c, &r("Bitch, Don't Kill My Vibe", &["Kendrick Lamar"], Some(310_000))), Reason::NoCandidate);
}

#[test]
fn short_files_never_auto() {
    let mut c = library(&[]);
    rejected(&run(&mut c, &r("Cha-Ching", &[], Some(1_500))), Reason::LowConfidence);
}

#[test]
fn isrc_with_wrong_duration_goes_to_review() {
    let mut c = library(&[]);
    let mut t = r("Falling", &["MQX"], Some(200_000));
    t.isrc = Some("QZFP52231004".into());
    let res = run(&mut c, &t);
    rejected(&res, Reason::DurationMismatch);
    assert!(res.needs_review());
}

#[test]
fn different_isrc_blocks_a_fuzzy_match() {
    let mut c = library(&[]);
    let mut t = r("Into You", &["Ariana Grande"], Some(244_000));
    t.isrc = Some("GBAAA2600001".into());
    let res = run(&mut c, &t);
    assert!(res.accepted.is_none(), "{res:#?}");
    assert!(res.needs_review());
}

#[test]
fn no_duration_or_spotify_account_data_never_auto() {
    let mut c = library(&[]);
    let res = run(&mut c, &r("Into You", &["Ariana Grande"], None));
    rejected(&res, Reason::LowConfidence);
    assert_eq!(res.candidates[0].path, path(r"02-11 - Ariana Grande - Into You.flac"), "best candidate first for review");
    let res = run_with(&mut c, &r("Into You", &["Ariana Grande"], Some(244_000)), policy_for("spotify-json"));
    rejected(&res, Reason::LowConfidence);
}

#[test]
fn duplicates_are_ambiguous() {
    let mut c = library(&[(r"copy\Into You.flac", "Into You", "Ariana Grande", 244.4, "USUM71601827", "")]);
    rejected(&run(&mut c, &r("Into You", &["Ariana Grande"], Some(244_000))), Reason::Ambiguous);
    let mut t = r("Into You", &["Ariana Grande"], Some(244_000));
    t.isrc = Some("USUM71601827".into());
    let res = run(&mut c, &t);
    rejected(&res, Reason::Ambiguous);
    assert_eq!(res.candidates.len(), 2);
}

#[test]
fn absent_songs_have_no_candidate() {
    let mut c = library(&[]);
    rejected(&run(&mut c, &r("Bohemian Rhapsody", &["Queen"], Some(354_000))), Reason::NoCandidate);
    rejected(&run(&mut c, &r("", &[], None)), Reason::NoCandidate);
}

// ---- paths ------------------------------------------------------------------------------------

#[test]
fn exact_and_suffix_paths() {
    let mut c = library(&[]);
    let mut t = r("", &[], None);
    t.location = Some(path(r"mqx\01 - FALLING.flac"));
    accepted(&run(&mut c, &t), Tier::Path, r"MQX\01 - Falling.flac");

    // A playlist from the phone: different root, same folder + file name.
    t.location = Some(r"\storage\emulated\0\Music\MQX\01 - Falling.flac".into());
    t.duration_ms = Some(155_000);
    accepted(&run(&mut c, &t), Tier::PathSuffix, r"MQX\01 - Falling.flac");

    // …but not when the playlist's duration contradicts it.
    t.duration_ms = Some(300_000);
    rejected(&run(&mut c, &t), Reason::DurationMismatch);

    // Same file name in another folder: a suggestion only.
    t.location = Some(r"\storage\emulated\0\Download\01 - Falling.flac".into());
    t.duration_ms = None;
    let res = run(&mut c, &t);
    rejected(&res, Reason::LowConfidence);
    assert!(res.needs_review());

    // NFC vs NFD file names are the same file.
    t.location = Some(path("Frozy - From the Islands (kompa pasión).flac"));
    accepted(&run(&mut c, &t), Tier::Path, "Frozy - From the Islands (kompa pasio\u{301}n).flac");
}

// ---- decisions --------------------------------------------------------------------------------

#[test]
fn decisions_are_never_asked_twice() {
    let mut c = library(&[]);
    let t = r("Swimming Pools (Drank)", &["Kendrick Lamar"], Some(221_000));
    let key = ref_key(&t);
    db::set_match_decision(&c, &key, "missing", None).unwrap();
    let res = run(&mut c, &t);
    rejected(&res, Reason::MarkedMissing);
    assert!(!res.needs_review());

    db::set_match_decision(&c, &key, "accept", Some(&path(r"09 Swimming Pools (Drank) [Extended].flac"))).unwrap();
    accepted(&run(&mut c, &t), Tier::Decision, r"09 Swimming Pools (Drank) [Extended].flac");

    db::set_match_decision(&c, &key, "accept", Some(r"C:\gone.flac")).unwrap();
    let res = run(&mut c, &t);
    assert!(res.accepted.is_none() && res.explanation.starts_with("The file you chose earlier"), "{res:#?}");
}

#[test]
fn ref_keys_are_stable_and_distinct() {
    let a = r("Into You", &["Ariana Grande"], Some(244_400));
    let b = r("Into You", &["Ariana Grande"], Some(244_900));
    assert_eq!(ref_key(&a), ref_key(&b), "same second");
    assert_ne!(ref_key(&a), ref_key(&r("Into You", &["Ariana Grande"], Some(180_000))));
    let mut s = a.clone();
    s.service_ids.insert("spotify".into(), "6tDDoYIxWvMLTdKpjFkc1B".into());
    assert_eq!(ref_key(&s), "spotify:6tDDoYIxWvMLTdKpjFkc1B");
}

// ---- index maintenance + report ---------------------------------------------------------------

#[test]
fn fts_index_is_incremental() {
    let mut c = library(&[]);
    assert_eq!(sync_fts(&mut c).unwrap(), TRACKS.len());
    assert_eq!(sync_fts(&mut c).unwrap(), 0, "nothing changed");
    c.execute("UPDATE tracks SET title = 'Falling (Live)', mtime = 2 WHERE title = 'Falling'", []).unwrap();
    assert_eq!(sync_fts(&mut c).unwrap(), 1, "re-read track re-indexed");
    c.execute("DELETE FROM tracks WHERE title = 'Heartbeat'", []).unwrap();
    sync_fts(&mut c).unwrap();
    let n: i64 = c.query_row("SELECT COUNT(*) FROM track_match_fts", [], |r| r.get(0)).unwrap();
    assert_eq!(n as usize, TRACKS.len() - 1, "removed track dropped");
    // The renamed track is now a live version: the studio title no longer auto-matches it.
    rejected(&run(&mut c, &r("Falling", &["MQX"], Some(155_000))), Reason::VersionMismatch);
}

#[test]
fn unmatched_report_lists_reasons() {
    let mut c = library(&[]);
    let refs = [r("Into You", &["Ariana Grande"], Some(244_000)), r("Swimming Pools (Drank)", &["Kendrick Lamar"], Some(221_000)), r("=cmd()", &["x"], None)];
    let m = Matcher::open(&mut c).unwrap();
    let rows: Vec<_> = refs.iter().enumerate().map(|(i, t)| (i + 1, t.clone(), m.match_ref(&c, t, None, Policy { fuzzy_auto: true }).unwrap())).collect();
    let csv = unmatched_report(&rows);
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 3, "header + the two not accepted:\n{csv}");
    assert!(lines[1].starts_with("2,Swimming Pools (Drank),Kendrick Lamar,,221000,,,version mismatch,"), "{csv}");
    assert!(lines[2].starts_with("3,'=cmd(),x,"), "formula guarded: {csv}");
}
