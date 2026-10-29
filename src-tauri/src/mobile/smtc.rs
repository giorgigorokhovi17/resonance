//! The Android media session's side of the engine (docs/android-design.md §2): the same updates
//! the Windows media overlay gets, handed to Kotlin over JNI (NativeBridge.kt), and the commands
//! from the notification, lock screen and Bluetooth buttons handed back to the engine.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use crossbeam_channel::{unbounded, Sender};
use jni::objects::{GlobalRef, JClass, JObject, JString, JValue};
use jni::sys::jdouble;
use jni::{JNIEnv, JavaVM};
use tauri::{AppHandle, Manager};

use crate::audio::engine::{Command, Engine};
use crate::audio::types::PlayState;

#[derive(Clone)]
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

struct Bridge {
    vm: JavaVM,
    /// app.resonance.player.NativeBridge; Rust's own threads can't look app classes up by name.
    class: GlobalRef,
}

static BRIDGE: OnceLock<Bridge> = OnceLock::new();
static ENGINE: OnceLock<Arc<Engine>> = OnceLock::new();
static APP: OnceLock<AppHandle> = OnceLock::new();
/// The last song and play state, sent again when Kotlin registers (it may come after them).
static LAST: Mutex<(Option<SmtcUpdate>, Option<SmtcUpdate>)> = Mutex::new((None, None));

/// Starts passing updates to the media session. Kotlin registers itself when the session starts.
pub fn start(engine: Arc<Engine>, app: AppHandle) -> Option<Sender<SmtcUpdate>> {
    let _ = ENGINE.set(engine);
    let _ = APP.set(app);
    let (tx, rx) = unbounded::<SmtcUpdate>();
    std::thread::Builder::new()
        .name("media-session".into())
        .spawn(move || {
            while let Ok(update) = rx.recv() {
                {
                    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
                    match &update {
                        SmtcUpdate::Track { .. } => last.0 = Some(update.clone()),
                        SmtcUpdate::Playback { .. } => last.1 = Some(update.clone()),
                    }
                }
                publish(&update);
            }
        })
        .ok()?;
    Some(tx)
}

/// Runs `f` with a JNI environment and the NativeBridge class, on any thread. None until Kotlin
/// has registered, or when the call fails (a Java exception is logged and cleared).
pub fn with_bridge<T>(f: impl FnOnce(&mut JNIEnv, &JClass) -> jni::errors::Result<T>) -> Option<T> {
    let bridge = BRIDGE.get()?;
    let mut env = bridge.vm.attach_current_thread_permanently().ok()?;
    let class: &JClass = bridge.class.as_obj().into();
    // A frame per call: threads stay attached, so local references must not pile up.
    let res = env.with_local_frame(16, |env| f(env, class));
    if let Err(e) = &res {
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_describe();
            let _ = env.exception_clear();
        }
        log::warn!("call into the app failed: {e}");
    }
    res.ok()
}

/// Calls a static `()V` method of NativeBridge (e.g. "pickMusicFolder").
pub fn call_static(method: &str) -> Result<(), String> {
    with_bridge(|env, class| env.call_static_method(class, method, "()V", &[]).map(|_| ()))
        .ok_or_else(|| format!("{method} isn't available yet."))
}

fn publish(update: &SmtcUpdate) {
    let Some(bridge) = BRIDGE.get() else { return };
    let Ok(mut env) = bridge.vm.attach_current_thread_permanently() else { return };
    // A frame per call: this thread stays attached, so local references must not pile up.
    let res = env.with_local_frame(8, |env| -> jni::errors::Result<()> {
        let class: &JClass = bridge.class.as_obj().into();
        match update {
            SmtcUpdate::Track { title, artist, album, cover, duration } => {
                let title = env.new_string(title)?;
                let artist = env.new_string(artist)?;
                let album = env.new_string(album)?;
                let cover = match cover {
                    Some(p) => JObject::from(env.new_string(p.to_string_lossy())?),
                    None => JObject::null(),
                };
                env.call_static_method(
                    class,
                    "onTrack",
                    "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;D)V",
                    &[
                        JValue::Object(&title),
                        JValue::Object(&artist),
                        JValue::Object(&album),
                        JValue::Object(&cover),
                        JValue::Double(*duration),
                    ],
                )?;
            }
            SmtcUpdate::Playback { state, position } => {
                // NativeBridge.STOPPED / PLAYING / PAUSED
                let code = match state {
                    PlayState::Stopped => 0,
                    PlayState::Playing => 1,
                    PlayState::Paused => 2,
                };
                env.call_static_method(class, "onPlayback", "(ID)V", &[JValue::Int(code), JValue::Double(*position)])?;
            }
        }
        Ok(())
    });
    if res.is_err() {
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_describe();
            let _ = env.exception_clear();
        }
        log::warn!("media session update failed: {res:?}");
    }
}

/// NativeBridge.register(): remembers the class and sends the current state.
#[no_mangle]
pub extern "system" fn Java_app_resonance_player_NativeBridge_register(env: JNIEnv, class: JClass) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if BRIDGE.get().is_none() {
            let (Ok(vm), Ok(class)) = (env.get_java_vm(), env.new_global_ref(&class)) else { return };
            let _ = BRIDGE.set(Bridge { vm, class });
        }
        let (track, playback) = LAST.lock().unwrap_or_else(|e| e.into_inner()).clone();
        for update in [track, playback].into_iter().flatten() {
            publish(&update);
        }
    }));
}

/// MainActivity.onCreate. When the screen was closed (swiped away) while music played on, the
/// window went with it, and a new screen in the same process doesn't get one back by itself:
/// build it again. tao gives it to the activity that has no window yet.
#[no_mangle]
pub extern "system" fn Java_app_resonance_player_NativeBridge_activityCreated(_env: JNIEnv, _class: JClass) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let Some(app) = APP.get() else { return }; // first start: Tauri opens the window itself
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            if handle.get_webview_window("main").is_some() {
                return;
            }
            let built = handle
                .config()
                .app
                .windows
                .first()
                .ok_or(tauri::Error::WindowNotFound)
                .and_then(|cfg| tauri::WebviewWindowBuilder::from_config(&handle, cfg)?.build());
            match built {
                Ok(_) => log::info!("reopened the main window"),
                Err(e) => log::error!("cannot reopen the main window: {e}"),
            }
        });
    }));
}

/// NativeBridge.command(name, value): play, pause, toggle, next, previous, stop, seek.
#[no_mangle]
pub extern "system" fn Java_app_resonance_player_NativeBridge_command(
    mut env: JNIEnv,
    _class: JClass,
    name: JString,
    value: jdouble,
) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let Ok(name) = env.get_string(&name).map(String::from) else { return };
        let command = match name.as_str() {
            "play" => Command::Play,
            "pause" => Command::Pause,
            "toggle" => Command::Toggle,
            "next" => Command::Next,
            "previous" => Command::Previous,
            "stop" => Command::Stop,
            "seek" if value.is_finite() => Command::Seek(value.max(0.0)),
            _ => return,
        };
        if let Some(engine) = ENGINE.get() {
            engine.send(command);
        }
    }));
}
