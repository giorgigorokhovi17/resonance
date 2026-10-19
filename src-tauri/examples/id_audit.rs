//! Read-only audit of the identifiers a library carries, for playlist matching:
//! `cargo run --release --example id_audit -- "D:\Music" [more folders...]`
//!
//! Counts ISRC, MusicBrainz recording IDs and usable durations as lofty reads them, and cross-checks
//! with a raw byte scan of each file's head so mis-mapped or non-standard tag keys still show up.
//! Files are only read.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::ItemKey;

const AUDIO_EXT: &[&str] = &["flac", "mp3", "m4a", "mp4", "aac", "ogg", "oga", "opus", "wav", "aif", "aiff", "alac"];

#[derive(Default)]
struct Row {
    files: usize,
    tag_read_failed: usize,
    isrc: usize,
    isrc_valid: usize,
    mbid_recording: usize,
    mbid_valid: usize,
    mbid_track: usize,
    raw_isrc: usize,
    raw_mbid: usize,
    duration_ok: usize,
    duration_mismatch: usize,
    title: usize,
    artist: usize,
    album: usize,
    multi_artist: usize,
}

fn valid_isrc(s: &str) -> bool {
    let s: String = s.chars().filter(|c| *c != '-').collect::<String>().to_ascii_uppercase();
    s.len() == 12
        && s[..2].chars().all(|c| c.is_ascii_alphabetic())
        && s[2..5].chars().all(|c| c.is_ascii_alphanumeric())
        && s[5..].chars().all(|c| c.is_ascii_digit())
}

fn valid_uuid(s: &str) -> bool {
    let s = s.trim();
    s.len() == 36
        && s.char_indices().all(|(i, c)| if [8, 13, 18, 23].contains(&i) { c == '-' } else { c.is_ascii_hexdigit() })
}

/// Case-insensitive search for tag keys in the first 2 MiB (Vorbis comments, ID3v2, MP4 `moov` when
/// it is at the front). Catches keys lofty doesn't map.
fn raw_scan(path: &Path) -> (bool, bool) {
    let mut buf = Vec::new();
    if let Ok(f) = std::fs::File::open(path) {
        let _ = f.take(2 * 1024 * 1024).read_to_end(&mut buf);
    }
    let lower: Vec<u8> = buf.iter().map(|b| b.to_ascii_lowercase()).collect();
    let has = |n: &[u8]| lower.windows(n.len()).any(|w| w == n);
    (has(b"isrc") || has(b"tsrc"), has(b"musicbrainz_trackid") || has(b"musicbrainz track id") || has(b"http://musicbrainz.org"))
}

fn main() {
    let roots: Vec<String> = std::env::args().skip(1).collect();
    assert!(!roots.is_empty(), "usage: id_audit <folder> [folder...]");
    let mut by_codec: BTreeMap<String, Row> = BTreeMap::new();
    let mut samples: Vec<String> = Vec::new();

    for root in &roots {
        for e in walkdir::WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
            let p = e.path();
            let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase();
            if !e.file_type().is_file() || !AUDIO_EXT.contains(&ext.as_str()) {
                continue;
            }
            let row = by_codec.entry(ext.clone()).or_default();
            row.files += 1;
            let (raw_i, raw_m) = raw_scan(p);
            row.raw_isrc += raw_i as usize;
            row.raw_mbid += raw_m as usize;

            let decoded = resonance_lib::audio::decoder::probe_info(p).map(|i| i.duration);
            let tagged = match Probe::open(p).and_then(|x| x.guess_file_type().map_err(Into::into)).and_then(|x| x.read()) {
                Ok(t) => t,
                Err(_) => {
                    row.tag_read_failed += 1;
                    continue;
                }
            };
            let lofty_dur = tagged.properties().duration().as_secs_f64();
            if lofty_dur > 1.0 {
                row.duration_ok += 1;
            }
            if let Some(d) = decoded {
                if (d - lofty_dur).abs() > 1.0 {
                    row.duration_mismatch += 1;
                }
            }

            let get = |k: ItemKey| tagged.tags().iter().find_map(|t| t.get_string(k).map(|s| s.trim().to_string())).filter(|s| !s.is_empty());
            let isrc = get(ItemKey::Isrc);
            let mbid = get(ItemKey::MusicBrainzRecordingId);
            if let Some(i) = &isrc {
                row.isrc += 1;
                row.isrc_valid += valid_isrc(i) as usize;
            }
            if let Some(m) = &mbid {
                row.mbid_recording += 1;
                row.mbid_valid += valid_uuid(m) as usize;
            }
            row.mbid_track += get(ItemKey::MusicBrainzTrackId).is_some() as usize;
            row.title += get(ItemKey::TrackTitle).is_some() as usize;
            let artist = get(ItemKey::TrackArtist);
            row.artist += artist.is_some() as usize;
            row.album += get(ItemKey::AlbumTitle).is_some() as usize;
            let multi = tagged.tags().iter().any(|t| t.get_strings(ItemKey::TrackArtist).count() > 1)
                || artist.as_deref().is_some_and(|a| a.contains(';') || a.contains(" feat") || a.contains(" ft.") || a.contains(" & "));
            row.multi_artist += multi as usize;
            if samples.len() < 12 {
                samples.push(format!(
                    "{} | artist={:?} title={:?} isrc={:?} mbid={:?} dur={:.1}s decoded={:?}",
                    p.file_name().unwrap().to_string_lossy(),
                    artist,
                    get(ItemKey::TrackTitle),
                    isrc,
                    mbid,
                    lofty_dur,
                    decoded.map(|d| (d * 10.0).round() / 10.0)
                ));
            }
        }
    }

    println!(
        "{:<6} {:>5} {:>6} {:>5} {:>7} {:>5} {:>7} {:>6} {:>6} {:>6} {:>6} {:>5} {:>6} {:>5} {:>5}",
        "ext", "files", "noTags", "ISRC", "valid", "MBID", "valid", "relTID", "rawISR", "rawMB", "dur>1s", "dMis", "title", "artst", "multi"
    );
    let mut total = Row::default();
    for (ext, r) in &by_codec {
        println!(
            "{:<6} {:>5} {:>6} {:>5} {:>7} {:>5} {:>7} {:>6} {:>6} {:>6} {:>6} {:>5} {:>6} {:>5} {:>5}",
            ext, r.files, r.tag_read_failed, r.isrc, r.isrc_valid, r.mbid_recording, r.mbid_valid, r.mbid_track,
            r.raw_isrc, r.raw_mbid, r.duration_ok, r.duration_mismatch, r.title, r.artist, r.multi_artist
        );
        total.files += r.files;
        total.tag_read_failed += r.tag_read_failed;
        total.isrc += r.isrc;
        total.isrc_valid += r.isrc_valid;
        total.mbid_recording += r.mbid_recording;
        total.mbid_valid += r.mbid_valid;
        total.mbid_track += r.mbid_track;
        total.raw_isrc += r.raw_isrc;
        total.raw_mbid += r.raw_mbid;
        total.duration_ok += r.duration_ok;
        total.duration_mismatch += r.duration_mismatch;
        total.title += r.title;
        total.artist += r.artist;
        total.multi_artist += r.multi_artist;
        total.album += r.album;
    }
    let r = &total;
    println!(
        "{:<6} {:>5} {:>6} {:>5} {:>7} {:>5} {:>7} {:>6} {:>6} {:>6} {:>6} {:>5} {:>6} {:>5} {:>5}",
        "TOTAL", r.files, r.tag_read_failed, r.isrc, r.isrc_valid, r.mbid_recording, r.mbid_valid, r.mbid_track,
        r.raw_isrc, r.raw_mbid, r.duration_ok, r.duration_mismatch, r.title, r.artist, r.multi_artist
    );
    println!("albums tagged: {}/{}", r.album, r.files);
    println!("\nsamples:");
    for s in samples {
        println!("  {s}");
    }
}
