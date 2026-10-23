//! Headless engine harness: `cargo run --example play -- <shared|exclusive> <seconds> <files...>`
//! Plays the files as a queue through the real output device and prints engine events, so the
//! audio path (gapless boundaries, seeking, format negotiation) can be verified without the UI.

use std::path::Path;
use std::time::{Duration, Instant};

use resonance_lib::audio::engine::{Command, Engine, EngineConfig, EngineEvent};
use resonance_lib::audio::eq::EqSettings;
use resonance_lib::audio::types::{OutputMode, ReplayGainMode};
use resonance_lib::library::tags::read_track;
use resonance_lib::queue::PlayQueue;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = match args.first().map(|s| s.as_str()) {
        Some("exclusive") => OutputMode::Exclusive,
        _ => OutputMode::Shared,
    };
    let secs: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(10.0);
    let seek_test = std::env::var("SEEK_TEST").is_ok();
    let files = &args[2..];

    let mut queue = PlayQueue::default();
    let tracks = files.iter().map(|f| read_track(Path::new(f), false).unwrap().0).collect();
    let first = queue.replace(tracks, 0).expect("no files");

    let start = Instant::now();
    let engine = Engine::new(
        EngineConfig {
            mode,
            device_id: None,
            volume: 1.0,
            eq: EqSettings::default(),
            rg_mode: ReplayGainMode::Off,
            rg_preamp: 0.0,
            crossfade: 0.0,
        },
        queue,
        move |ev| {
            let t = start.elapsed().as_secs_f64();
            match ev {
                EngineEvent::Status(s) => {
                    let out = s.output.as_ref().map(|o| {
                        format!("{:?} {} {}Hz {}bit/{} ch{}", o.mode, o.device_name, o.sample_rate, o.bits, o.container_bits, o.channels)
                    });
                    let src = s.source.as_ref().map(|f| format!("{} {}Hz {:?}bit", f.codec, f.sample_rate, f.bits_per_sample));
                    println!(
                        "[{t:6.2}] {:?} pos={:.2}/{:.2} uid={:?} src={:?} out={:?} bitperfect={} resampled={} underruns={}",
                        s.state, s.position, s.duration, s.current_uid, src, out, s.bit_perfect, s.resampled, s.underruns
                    );
                }
                EngineEvent::TrackChanged(item) => {
                    println!("[{t:6.2}] TRACK -> {:?}", item.map(|i| i.track.title));
                }
                EngineEvent::Error(e) => println!("[{t:6.2}] ERROR {e}"),
            }
        },
    );
    engine.send(Command::PlayItem { uid: first.uid, start: 0.0, paused: false });
    if seek_test {
        std::thread::sleep(Duration::from_millis(1500));
        println!("--- seek to 4.0");
        engine.send(Command::Seek(4.0));
        std::thread::sleep(Duration::from_millis(1000));
        println!("--- pause");
        engine.send(Command::Pause);
        std::thread::sleep(Duration::from_millis(700));
        println!("--- play");
        engine.send(Command::Play);
    }
    std::thread::sleep(Duration::from_secs_f64(secs));
    println!("final: {:?} pos={:.2} underruns={}", engine.status().state, engine.status().position, engine.status().underruns);
    engine.shutdown();
}
