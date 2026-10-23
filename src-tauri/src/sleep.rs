//! Sleep timer: pause after a number of minutes (fading out over the last few seconds), or stop
//! when the playing song ends.

use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::audio::dsp::slider_to_gain;
use crate::audio::engine::{Command, Engine};
use crate::audio::types::PlayState;

/// The volume fades out over this long before the timer pauses playback.
const FADE: Duration = Duration::from_secs(10);
pub const MAX_MINUTES: u32 = 12 * 60;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Off,
    At(Instant),
    EndOfSong { started: bool },
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SleepStatus {
    /// "off" | "minutes" | "endOfSong"
    pub mode: &'static str,
    /// Seconds left, for "minutes".
    pub remaining: Option<f64>,
}

pub struct SleepTimer {
    mode: Mutex<Mode>,
    /// The volume is turned down by the fade.
    fading: Mutex<bool>,
    engine: Arc<Engine>,
    app: AppHandle,
}

impl SleepTimer {
    pub fn new(engine: Arc<Engine>, app: AppHandle) -> Arc<Self> {
        let t = Arc::new(Self { mode: Mutex::new(Mode::Off), fading: Mutex::new(false), engine, app });
        let timer = t.clone();
        let _ = std::thread::Builder::new().name("sleep-timer".into()).spawn(move || loop {
            std::thread::sleep(Duration::from_millis(250));
            timer.tick();
        });
        t
    }

    pub fn status(&self) -> SleepStatus {
        match *self.mode.lock() {
            Mode::Off => SleepStatus { mode: "off", remaining: None },
            Mode::At(at) => SleepStatus {
                mode: "minutes",
                remaining: Some(at.saturating_duration_since(Instant::now()).as_secs_f64()),
            },
            Mode::EndOfSong { .. } => SleepStatus { mode: "endOfSong", remaining: None },
        }
    }

    /// `minutes` > 0 pauses after that long; `end_of_song` stops when the playing song ends;
    /// neither turns the timer off.
    pub fn set(&self, minutes: Option<u32>, end_of_song: bool) -> SleepStatus {
        let mode = match (minutes.filter(|m| *m > 0), end_of_song) {
            (_, true) => Mode::EndOfSong { started: self.engine.status().state != PlayState::Stopped },
            (Some(m), false) => Mode::At(Instant::now() + Duration::from_secs(u64::from(m.min(MAX_MINUTES)) * 60)),
            (None, false) => Mode::Off,
        };
        *self.mode.lock() = mode;
        self.restore_volume();
        self.engine.send(Command::StopAfterCurrent(matches!(mode, Mode::EndOfSong { .. })));
        self.changed()
    }

    fn changed(&self) -> SleepStatus {
        let s = self.status();
        let _ = self.app.emit("sleep:changed", &s);
        s
    }

    fn restore_volume(&self) {
        let mut fading = self.fading.lock();
        if *fading {
            *fading = false;
            self.engine.set_volume(self.engine.volume());
        }
    }

    fn tick(&self) {
        let mode = *self.mode.lock();
        match mode {
            Mode::Off => {}
            Mode::At(at) => {
                let left = at.saturating_duration_since(Instant::now());
                let playing = self.engine.status().state == PlayState::Playing;
                if left.is_zero() {
                    *self.mode.lock() = Mode::Off;
                    if playing {
                        self.engine.send(Command::Pause);
                        // Let the pause take effect before the volume comes back.
                        std::thread::sleep(Duration::from_millis(400));
                    }
                    self.restore_volume();
                    self.changed();
                } else if left < FADE && playing {
                    // Turn the volume down without moving the volume slider.
                    let k = left.as_secs_f64() / FADE.as_secs_f64();
                    *self.fading.lock() = true;
                    self.engine.shared.set_volume(slider_to_gain(self.engine.volume()) * k * k);
                }
            }
            Mode::EndOfSong { started } => {
                let stopped = self.engine.status().state == PlayState::Stopped;
                if !stopped && !started {
                    *self.mode.lock() = Mode::EndOfSong { started: true };
                } else if stopped && started {
                    // The song ended (or playback was stopped): the timer is done.
                    *self.mode.lock() = Mode::Off;
                    self.engine.send(Command::StopAfterCurrent(false));
                    self.changed();
                }
            }
        }
    }
}
