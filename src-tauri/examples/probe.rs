//! Debug helper: `cargo run --example probe -- <file>` decodes a whole file and prints its format.
use resonance_lib::audio::decoder::Decoder;

fn main() {
    env_logger::init();
    let path = std::env::args().nth(1).expect("file");
    let mut d = match Decoder::open(std::path::Path::new(&path)) {
        Ok(d) => d,
        Err(e) => {
            println!("open failed: {e:#}");
            let f = std::fs::File::open(&path).unwrap();
            let mss = symphonia::core::io::MediaSourceStream::new(Box::new(f), Default::default());
            let r = symphonia::default::get_probe().probe(
                &symphonia::core::formats::probe::Hint::new(),
                mss,
                Default::default(),
                Default::default(),
            );
            println!("raw probe with defaults: {:?}", r.map(|f| f.format_info().short_name));
            return;
        }
    };
    println!("{:?} duration={:?}", d.source, d.duration_secs);
    let mut frames = 0usize;
    while let Some(c) = d.next_chunk().unwrap() {
        frames += c.len() / d.channels();
    }
    println!("decoded frames: {frames} ({:.3}s)", frames as f64 / d.sample_rate() as f64);
}
