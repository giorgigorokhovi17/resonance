//! Import public links into a COPY of a library database and show how each song matched:
//! `cargo run --example link_import_check -- <library.db copy> <link> [<link>...]`
use resonance_lib::library::{db, interchange};
use resonance_lib::sources::links;

fn fname(p: &str) -> String {
    std::path::Path::new(p).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let db_path = args.next().expect("library.db copy");
    let mut conn = db::open(std::path::Path::new(&db_path)).expect("open db");
    for url in args {
        let link = links::parse_link(&url).expect("supported link");
        let f = links::fetch(&link).await.expect("fetch");
        let out = interchange::import_into_db(&mut conn, &f.playlist, std::path::Path::new(".")).expect("import");
        println!("== {} \"{}\": {} songs, matched {:?}, review {}, note {:?}", link.info().service, out.name, out.total, out.matched, out.review, f.note);
        for (pos, r, m) in interchange::rematch_playlist(&mut conn, out.id).unwrap() {
            let what = match &m.accepted {
                Some(c) => format!("MATCHED ({:?}) -> {}", m.tier.unwrap(), fname(&c.path)),
                None if !m.candidates.is_empty() => format!("REVIEW {} -> {}", m.reason.name(), fname(&m.candidates[0].path)),
                None => format!("missing ({})", m.reason.name()),
            };
            if m.accepted.is_some() || !m.candidates.is_empty() || pos <= 3 {
                println!("  #{pos:<3} {:<45} {}", format!("{} · {}", r.title, r.artists.join(", ")).chars().take(45).collect::<String>(), what);
            }
        }
        let rv = interchange::playlist_review(&mut conn, out.id).unwrap();
        println!("  review page: {} pending, {} missing, {} matched", rv.pending.len(), rv.missing.len(), rv.matched);
    }
}
