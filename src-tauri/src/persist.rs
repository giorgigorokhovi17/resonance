//! Saves the play queue (and position) between sessions.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::audio::engine::Engine;
use crate::library::Library;
use crate::queue::{PlayQueue, RepeatMode};

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct SavedQueue {
    paths: Vec<String>,
    current: Option<usize>,
    position: f64,
    shuffle: bool,
    repeat: RepeatMode,
}

pub fn save_queue(file: &Path, engine: &Engine) {
    let snap = engine.queue.lock().snapshot();
    let status = engine.status();
    let current = snap.current_uid.and_then(|u| snap.items.iter().position(|i| i.uid == u));
    let position = if status.current_uid.is_some() && status.current_uid == snap.current_uid { status.position } else { 0.0 };
    let saved = SavedQueue {
        paths: snap.items.into_iter().map(|i| i.track.path).collect(),
        current,
        position,
        shuffle: snap.shuffle,
        repeat: snap.repeat,
    };
    let tmp = file.with_extension("json.tmp");
    if let Ok(bytes) = serde_json::to_vec(&saved) {
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, file);
        }
    }
}

/// Returns the restored queue and (current index, position) to resume from.
pub fn load_queue(file: &Path, library: &Library) -> (PlayQueue, Option<f64>) {
    let Some(saved) = std::fs::read(file).ok().and_then(|b| serde_json::from_slice::<SavedQueue>(&b).ok()) else {
        return (PlayQueue::default(), None);
    };
    let resolved = library.resolve_paths(&saved.paths);
    // Drop files that disappeared, keeping the current pointer on the same track.
    let mut current = None;
    let mut tracks = Vec::with_capacity(resolved.len());
    for (i, (t, missing)) in resolved.into_iter().enumerate() {
        if missing {
            continue;
        }
        if Some(i) == saved.current {
            current = Some(tracks.len());
        }
        tracks.push(t);
    }
    let q = PlayQueue::restore(tracks, current, saved.shuffle, saved.repeat);
    let pos = current.map(|_| saved.position);
    (q, pos)
}
