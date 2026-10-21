//! Library scan benchmark: `cargo run --release --example scan -- <music dir> [<music dir>...]`
//! Uses a throwaway database/cover cache in the temp dir, scans twice (cold, then incremental).

use std::sync::Arc;
use std::time::Instant;

use resonance_lib::library::{Library, LibraryEvent, ScanRequest};

fn main() {
    let dirs: Vec<String> = std::env::args().skip(1).collect();
    let tmp = std::env::temp_dir().join(format!("resonance-scan-bench-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let (tx, rx) = crossbeam_channel::unbounded();
    let lib = Library::open(
        &tmp.join("library.db"),
        tmp.join("covers"),
        tmp.join("media"),
        Arc::new(move |ev| {
            if let LibraryEvent::Finished(s) = ev {
                let _ = tx.send(s);
            }
        }),
    )
    .unwrap();
    for d in &dirs {
        resonance_lib::library::db::add_folder(&lib.db.lock(), d).unwrap();
    }
    for pass in ["cold", "incremental"] {
        let t0 = Instant::now();
        lib.request_scan(ScanRequest::All { force: false });
        let s = rx.recv().unwrap();
        println!(
            "{pass:>12}: {:.2}s  total={} added={} updated={} removed={} errors={}",
            t0.elapsed().as_secs_f64(),
            s.total,
            s.added,
            s.updated,
            s.removed,
            s.errors
        );
    }
    let t0 = Instant::now();
    let tracks = lib.tracks().unwrap();
    println!("   load all: {} tracks in {} ms", tracks.len(), t0.elapsed().as_millis());
    println!(
        "   identifiers: {} with ISRC, {} with MusicBrainz recording id",
        tracks.iter().filter(|t| t.isrc.is_some()).count(),
        tracks.iter().filter(|t| t.mbid.is_some()).count()
    );
    let json = serde_json::to_vec(&tracks).unwrap();
    println!("   json size: {:.1} MB", json.len() as f64 / 1e6);
    let _ = std::fs::remove_dir_all(&tmp);
}
