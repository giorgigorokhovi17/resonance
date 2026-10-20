//! Read public playlist links the way the app does and print a summary:
//! `cargo run --example link_check -- <link> [<link>...]`
use resonance_lib::sources::links;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    for url in std::env::args().skip(1) {
        let Some(link) = links::parse_link(&url) else {
            println!("not a supported link: {url}");
            continue;
        };
        let t0 = std::time::Instant::now();
        match links::fetch(&link).await {
            Ok(f) => {
                let p = &f.playlist;
                let with_dur = p.items.iter().filter(|i| i.track.duration_ms.is_some()).count();
                let with_artist = p.items.iter().filter(|i| !i.track.artists.is_empty()).count();
                println!(
                    "{} {} \"{}\": {} tracks ({} with duration, {} with artist) in {:.1} s{}",
                    link.info().service, link.info().kind, p.name, p.items.len(), with_dur, with_artist,
                    t0.elapsed().as_secs_f64(), f.note.map(|n| format!(" — note: {n}")).unwrap_or_default()
                );
                for it in p.items.iter().take(2).chain(p.items.last()) {
                    let t = &it.track;
                    println!("    {:?} by {:?} ({:?} ms) {:?}", t.title, t.artists, t.duration_ms, t.service_ids);
                }
            }
            Err(e) => println!("{} {}: ERROR {e:#}", link.info().service, link.info().kind),
        }
    }
}
