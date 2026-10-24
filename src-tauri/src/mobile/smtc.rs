//! Windows media controls. On Android the media session takes this role (step 3 of
//! docs/android-design.md); until then nothing receives these updates.

#![allow(dead_code)]

use std::path::PathBuf;

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
