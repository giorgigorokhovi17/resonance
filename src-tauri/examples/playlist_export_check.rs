//! Export a real folder of music as M3U8, M3U, XSPF and CSV, read each file back, and check that
//! every entry resolves to the same existing file in the same order, with the same title and
//! artist, and (for the formats that carry them) the same ISRC and MusicBrainz id:
//! `cargo run --example playlist_export_check -- "D:\Music" <out dir>`
//!
//! Music files are only read. The playlists go to `<out dir>`, ready to open in VLC or foobar2000.

use std::path::{Path, PathBuf};

use resonance_lib::library::interchange::{self, Playlist, PlaylistItem};
use resonance_lib::library::model::is_audio_file;
use resonance_lib::library::{csv, m3u, tags, xspf};

fn main() {
    let mut args = std::env::args().skip(1);
    let music = PathBuf::from(args.next().expect("usage: playlist_export_check <music folder> <out dir>"));
    let out = PathBuf::from(args.next().expect("usage: playlist_export_check <music folder> <out dir>"));
    std::fs::create_dir_all(&out).unwrap();

    let mut files: Vec<PathBuf> = walkdir::WalkDir::new(&music)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && is_audio_file(e.path()))
        .map(|e| e.into_path())
        .collect();
    files.sort();
    let items: Vec<PlaylistItem> = files
        .iter()
        .map(|f| {
            let (t, _) = tags::read_track(f, false).expect("read tags");
            PlaylistItem { track: interchange::track_ref_from_track(&t), added_at: None }
        })
        .collect();
    let pl = Playlist { name: "Resonance export check".into(), source: "resonance".into(), source_id: None, items };
    let expected: Vec<String> = files.iter().map(|f| f.to_string_lossy().to_string()).collect();

    let mut failed = false;
    let with_isrc = pl.items.iter().filter(|i| i.track.isrc.is_some()).count();
    let with_mbid = pl.items.iter().filter(|i| i.track.mbid.is_some()).count();
    for name in ["export-check.m3u8", "export-check.m3u", "export-check.xspf", "export-check.csv"] {
        let file = out.join(name);
        let carries_ids = name.ends_with(".xspf") || name.ends_with(".csv");
        if name.ends_with(".xspf") {
            xspf::write_file(&file, &pl).unwrap();
        } else if name.ends_with(".csv") {
            csv::write_file(&file, &pl).unwrap();
        } else {
            m3u::write_file(&file, &pl).unwrap();
        }
        let back = interchange::parse_file(&file).unwrap();
        let base = file.parent().unwrap_or(Path::new("."));
        let got: Vec<String> = back.items.iter().filter_map(|i| interchange::resolve_location(i.track.location.as_deref()?, base)).collect();
        let exist = got.iter().filter(|p| Path::new(p).is_file()).count();
        let same_order = got == expected;
        let titles_ok = back.items.iter().zip(&pl.items).all(|(a, b)| a.track.title == b.track.title && a.track.artists == b.track.artists);
        for (a, b) in back.items.iter().zip(&pl.items) {
            if a.track.title != b.track.title || a.track.artists != b.track.artists {
                println!("    changed: {:?} {:?}  ->  {:?} {:?}", b.track.artists, b.track.title, a.track.artists, a.track.title);
            }
        }
        let ids_ok = !carries_ids
            || back.items.iter().zip(&pl.items).all(|(a, b)| a.track.isrc == b.track.isrc && a.track.mbid == b.track.mbid);
        let isrc_back = back.items.iter().filter(|i| i.track.isrc.is_some()).count();
        let mbid_back = back.items.iter().filter(|i| i.track.mbid.is_some()).count();
        let ok = same_order && exist == expected.len() && titles_ok && ids_ok;
        failed |= !ok;
        println!(
            "{:<18} entries {:>3}/{:<3} exist {:>3}  order {}  titles+artists {}  ISRC/MBID {}  {}  -> {}",
            name,
            got.len(),
            expected.len(),
            exist,
            if same_order { "same" } else { "DIFFERENT" },
            if titles_ok { "same" } else { "DIFFERENT" },
            if !carries_ids { "n/a".to_string() } else { format!("{isrc_back}/{mbid_back} {}", if ids_ok { "same" } else { "DIFFERENT" }) },
            if ok { "OK" } else { "FAIL" },
            file.display()
        );
    }
    let non_ascii = expected.iter().filter(|p| !p.is_ascii()).count();
    println!("{} files, {} with non-ASCII paths; tags give {with_isrc} valid ISRCs and {with_mbid} MusicBrainz ids", expected.len(), non_ascii);
    if failed {
        std::process::exit(1);
    }
}
