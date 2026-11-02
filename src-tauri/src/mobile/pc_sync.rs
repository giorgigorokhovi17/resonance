//! File sync with the PC on Android (docs/phone-file-sync.md). The sync itself is
//! `undertone_core::pc_sync`. This runs it on a thread, saves finished songs into
//! `Music/Undertone/Synced` through MediaStore, and keeps the process alive with a user-initiated
//! data transfer job (PcSyncJobService.kt).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use jni::objects::{JString, JValue};
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter, Manager};
use undertone_core::pc_sync::{self, Job, Link, Status};

use crate::AppState;

/// Where synced songs live on shared storage.
const ROOT: &str = "/storage/emulated/0/Music/Undertone/Synced";

static STATUS: Mutex<Option<Status>> = Mutex::new(None);
static CANCEL: AtomicBool = AtomicBool::new(false);

pub fn status() -> Status {
    STATUS.lock().clone().unwrap_or_default()
}

pub fn cancel() {
    CANCEL.store(true, Ordering::Relaxed);
}

fn report(app: &AppHandle, s: Status) {
    *STATUS.lock() = Some(s.clone());
    let _ = app.emit("pcsync:status", s);
}

fn staging_dir() -> Option<PathBuf> {
    super::smtc::with_bridge(|env, class| {
        let obj = env.call_static_method(class, "pcSyncStagingDir", "()Ljava/lang/String;", &[])?.l()?;
        if obj.is_null() {
            return Ok(None);
        }
        let s: String = env.get_string(&JString::from(obj))?.into();
        Ok(Some(PathBuf::from(s)))
    })
    .flatten()
}

fn publish(src: &Path, name: &str) -> Result<PathBuf, String> {
    let out = super::smtc::with_bridge(|env, class| {
        let (src, name) = (env.new_string(src.to_string_lossy())?, env.new_string(name)?);
        let out = env
            .call_static_method(
                class,
                "publishSynced",
                "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
                &[JValue::Object(&src), JValue::Object(&name)],
            )?
            .l()?;
        if out.is_null() {
            return Ok(None);
        }
        let text: String = env.get_string(&JString::from(out))?.into();
        Ok(Some(text))
    })
    .flatten()
    .ok_or("Couldn't save a song to Music.")?;
    match out.strip_prefix("ERR:") {
        Some(e) => Err(e.to_string()),
        None => Ok(PathBuf::from(out)),
    }
}

fn remove(path: &Path) -> bool {
    super::smtc::with_bridge(|env, class| {
        let path = env.new_string(path.to_string_lossy())?;
        env.call_static_method(class, "removeSynced", "(Ljava/lang/String;)Z", &[JValue::Object(&path)])?.z()
    })
    .unwrap_or(false)
}

fn keep_alive(running: bool) {
    let _ = super::smtc::with_bridge(|env, class| {
        env.call_static_method(class, "onPcSync", "(Z)V", &[JValue::Bool(running.into())]).map(|_| ())
    });
}

pub fn start(app: &AppHandle) -> Result<(), String> {
    let state = app.try_state::<AppState>().ok_or("The app is still starting.")?;
    let (library, settings) = (state.library.clone(), state.settings.get().pc_sync);
    let link = Link::parse(&settings.link)?;
    let staging = staging_dir().ok_or("The app's storage isn't ready yet.")?;
    {
        let mut current = STATUS.lock();
        if current.as_ref().is_some_and(|s| s.running) {
            return Ok(());
        }
        *current = Some(Status { running: true, ..Status::default() });
    }
    CANCEL.store(false, Ordering::Relaxed);
    keep_alive(true);

    let app = app.clone();
    let thread = std::thread::Builder::new().name("pc-sync".into()).spawn(move || {
        let progress = |step: &str, done: usize, total: usize| {
            report(&app, Status { running: true, step: step.into(), done, total, ..Status::default() });
        };
        let job = Job {
            lib: &library,
            link,
            lists: settings.lists,
            folders: settings.folders,
            send: settings.send,
            staging,
            root: PathBuf::from(ROOT),
            publish: &publish,
            remove: &remove,
            cancel: &CANCEL,
            status: &progress,
        };
        let outcome = pc_sync::run(&job);
        keep_alive(false);
        let _ = app.emit("playlists:changed", ());
        report(
            &app,
            match outcome {
                Ok(summary) => Status { summary: Some(summary), ..Status::default() },
                Err(error) => Status { error: Some(error), ..Status::default() },
            },
        );
    });
    if let Err(e) = thread {
        keep_alive(false);
        *STATUS.lock() = None;
        return Err(e.to_string());
    }
    Ok(())
}
