//! Windows System Media Transport Controls (media overlay + hardware media keys) via souvlaki.
//! Runs on its own thread; the rest of the app sends it updates through a channel.

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::{unbounded, Sender};
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig, SeekDirection};

use crate::audio::engine::{Command, Engine};
use crate::audio::types::PlayState;

pub enum SmtcUpdate {
    Track {
        title: String,
        artist: String,
        album: String,
        cover: Option<PathBuf>,
        duration: f64,
    },
    Playback {
        state: PlayState,
        position: f64,
    },
}

pub fn start(hwnd: isize, engine: Arc<Engine>, on_raise: impl Fn() + Send + 'static) -> Option<Sender<SmtcUpdate>> {
    let (tx, rx) = unbounded::<SmtcUpdate>();
    let (ready_tx, ready_rx) = crossbeam_channel::bounded::<bool>(1);
    std::thread::Builder::new()
        .name("smtc".into())
        .spawn(move || {
            let config = PlatformConfig {
                display_name: "Undertone",
                dbus_name: "resonance",
                hwnd: Some(hwnd as *mut c_void),
            };
            let mut controls = match MediaControls::new(config) {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("media controls unavailable: {e}");
                    let _ = ready_tx.send(false);
                    return;
                }
            };
            let eng = engine.clone();
            let attached = controls.attach(move |ev| {
                let cmd = match ev {
                    MediaControlEvent::Play => Some(Command::Play),
                    MediaControlEvent::Pause => Some(Command::Pause),
                    MediaControlEvent::Toggle => Some(Command::Toggle),
                    MediaControlEvent::Next => Some(Command::Next),
                    MediaControlEvent::Previous => Some(Command::Previous),
                    MediaControlEvent::Stop => Some(Command::Stop),
                    MediaControlEvent::SetPosition(MediaPosition(p)) => Some(Command::Seek(p.as_secs_f64())),
                    MediaControlEvent::Seek(dir) | MediaControlEvent::SeekBy(dir, _) => {
                        let pos = eng.status().position;
                        Some(Command::Seek(match dir {
                            SeekDirection::Forward => pos + 10.0,
                            SeekDirection::Backward => (pos - 10.0).max(0.0),
                        }))
                    }
                    MediaControlEvent::Raise => {
                        on_raise();
                        None
                    }
                    _ => None,
                };
                if let Some(c) = cmd {
                    eng.send(c);
                }
            });
            if let Err(e) = attached {
                log::warn!("media controls attach failed: {e}");
                let _ = ready_tx.send(false);
                return;
            }
            let _ = ready_tx.send(true);
            let _ = controls.set_playback(MediaPlayback::Stopped);
            while let Ok(update) = rx.recv() {
                let res = match update {
                    SmtcUpdate::Track { title, artist, album, cover, duration } => {
                        let cover_url = cover.map(|p| format!("file://{}", p.display()));
                        controls.set_metadata(MediaMetadata {
                            title: Some(&title),
                            artist: Some(&artist),
                            album: Some(&album),
                            cover_url: cover_url.as_deref(),
                            duration: Some(Duration::from_secs_f64(duration.max(0.0))),
                        })
                    }
                    SmtcUpdate::Playback { state, position } => {
                        let progress = Some(MediaPosition(Duration::from_secs_f64(position.max(0.0))));
                        controls.set_playback(match state {
                            PlayState::Playing => MediaPlayback::Playing { progress },
                            PlayState::Paused => MediaPlayback::Paused { progress },
                            PlayState::Stopped => MediaPlayback::Stopped,
                        })
                    }
                };
                if let Err(e) = res {
                    log::debug!("media controls update failed: {e}");
                }
            }
        })
        .ok()?;
    match ready_rx.recv_timeout(Duration::from_secs(3)) {
        Ok(true) => Some(tx),
        _ => None,
    }
}
