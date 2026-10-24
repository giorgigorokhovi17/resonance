//! Global shortcuts (Settings → Global shortcuts): play/pause, next, previous, volume and mute
//! from any app, games included. Windows gives the keys to Undertone (RegisterHotKey), so the app
//! in front doesn't get them.

use std::str::FromStr;

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::audio::engine::Command;
use crate::settings::HotkeySettings;
use crate::AppState;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    PlayPause,
    Next,
    Previous,
    VolumeUp,
    VolumeDown,
    Mute,
}

/// A shortcut that couldn't be set up, and why.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    pub action: Action,
    pub keys: String,
    pub message: String,
}

/// How much one press of a volume shortcut changes the volume slider.
const VOLUME_STEP: f64 = 0.05;

/// The shortcuts registered now (by id) and the ones that couldn't be.
static ACTIVE: Mutex<Vec<(u32, Action)>> = Mutex::new(Vec::new());
static PROBLEMS: Mutex<Vec<Problem>> = Mutex::new(Vec::new());

pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            let action = ACTIVE.lock().iter().find(|(id, _)| *id == shortcut.id()).map(|(_, a)| *a);
            if let Some(action) = action {
                run(app, action);
            }
        })
        .build()
}

fn bindings(s: &HotkeySettings) -> Vec<(Action, &str)> {
    [
        (Action::PlayPause, &s.play_pause),
        (Action::Next, &s.next),
        (Action::Previous, &s.previous),
        (Action::VolumeUp, &s.volume_up),
        (Action::VolumeDown, &s.volume_down),
        (Action::Mute, &s.mute),
    ]
    .into_iter()
    .filter_map(|(a, k)| k.as_deref().map(|k| (a, k)))
    .collect()
}

/// Registers the shortcuts in `s` in place of the ones before, and tells the app about any that
/// couldn't be (taken by Windows or another app, or not a valid shortcut). Call it off the main
/// thread: registering waits on it.
pub fn apply(app: &AppHandle, s: &HotkeySettings) {
    let gs = app.global_shortcut();
    if let Err(e) = gs.unregister_all() {
        log::warn!("could not clear the global shortcuts: {e}");
    }
    let mut active: Vec<(u32, Action)> = Vec::new();
    let mut problems = Vec::new();
    if s.enabled {
        for (action, keys) in bindings(s) {
            let problem = |message: &str| Problem { action, keys: keys.to_string(), message: message.to_string() };
            let Ok(shortcut) = Shortcut::from_str(keys) else {
                problems.push(problem("That isn't a key combination Windows can use."));
                continue;
            };
            if active.iter().any(|(id, _)| *id == shortcut.id()) {
                problems.push(problem("Already used by another shortcut here."));
                continue;
            }
            match gs.register(shortcut) {
                Ok(()) => active.push((shortcut.id(), action)),
                Err(e) => {
                    log::info!("global shortcut {keys} not available: {e}");
                    problems.push(problem("Windows or another app already uses it."));
                }
            }
        }
    }
    *ACTIVE.lock() = active;
    *PROBLEMS.lock() = problems.clone();
    let _ = app.emit("hotkeys:problems", problems);
}

/// Lets go of every shortcut for now (the problem list stays as it was).
pub fn suspend(app: &AppHandle) {
    let _ = app.global_shortcut().unregister_all();
    ACTIVE.lock().clear();
}

pub fn problems() -> Vec<Problem> {
    PROBLEMS.lock().clone()
}

fn run(app: &AppHandle, action: Action) {
    let Some(state) = app.try_state::<AppState>() else { return };
    let engine = &state.engine;
    match action {
        Action::PlayPause => engine.send(Command::Toggle),
        Action::Next => engine.send(Command::Next),
        Action::Previous => engine.send(Command::Previous),
        Action::VolumeUp | Action::VolumeDown => {
            let step = if action == Action::VolumeUp { VOLUME_STEP } else { -VOLUME_STEP };
            let v = ((engine.volume() + step) * 100.0).round() / 100.0;
            let v = v.clamp(0.0, 1.0);
            engine.set_volume(v);
            engine.set_muted(false);
            state.settings.update(|s| s.volume = v);
        }
        Action::Mute => engine.set_muted(!engine.shared.muted.load(std::sync::atomic::Ordering::Relaxed)),
    }
    if matches!(action, Action::VolumeUp | Action::VolumeDown | Action::Mute) {
        // Status updates may be paused with the music; the volume slider follows this one.
        let _ = app.emit("player:status", engine.status());
    }
}
