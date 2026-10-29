//! Soulseek on Android (docs/android-design.md §5):
//! - Downloads stage in app-specific external storage (`getExternalFilesDir("soulseek")`) and on
//!   completion publish into `Music/Undertone/Soulseek` through MediaStore (`IS_PENDING`).
//! - While downloads are queued or running, a user-initiated data transfer job
//!   (`SoulseekJobService.kt`) keeps the process alive.
//! - Soulseek signs in only while the app is visible or a download job is running.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use jni::objects::{JClass, JObject, JString, JValue};
use jni::sys::jboolean;
use jni::JNIEnv;

use crate::library::{Library, ScanRequest};
use crate::sources::soulseek::Soulseek;

/// Where finished Soulseek downloads live on shared storage.
pub const PUBLISHED_ROOT: &str = "/storage/emulated/0/Music/Undertone/Soulseek";

static SOULSEEK: OnceLock<Arc<Soulseek>> = OnceLock::new();
static APP_VISIBLE: AtomicBool = AtomicBool::new(false);
static JOB_RUNNING: AtomicBool = AtomicBool::new(false);

pub fn start(soulseek: Arc<Soulseek>, library: Arc<Library>) {
    let lib = library.clone();
    soulseek.set_on_publish(
        Arc::new(move |src, subfolder, name| {
            let published = publish_download(src, subfolder, name)?;
            lib.request_scan(ScanRequest::All { force: false });
            Ok(published)
        }),
        Arc::new(final_exists),
    );
    soulseek.set_on_queue_change(Arc::new(|active, wifi_only| {
        let _ = super::smtc::with_bridge(|env, class| {
            env.call_static_method(
                class,
                "onSoulseekQueue",
                "(IZ)V",
                &[JValue::Int(active as i32), JValue::Bool(wifi_only.into())],
            )
            .map(|_| ())
        });
    }));
    let _ = SOULSEEK.set(soulseek);
    sync_allowed();
}

/// App-specific external storage where `soulseek-rs` writes in-flight files before publishing.
pub fn staging_dir() -> Option<PathBuf> {
    super::smtc::with_bridge(|env, class| {
        let obj = env.call_static_method(class, "soulseekStagingDir", "()Ljava/lang/String;", &[])?.l()?;
        if obj.is_null() {
            return Ok(None);
        }
        let s: String = env.get_string(&JString::from(obj))?.into();
        Ok(Some(PathBuf::from(s)))
    })
    .flatten()
}

fn final_exists(subfolder: Option<&str>, name: &str) -> bool {
    let dir = match subfolder.filter(|s| !s.is_empty()) {
        Some(sub) => Path::new(PUBLISHED_ROOT).join(sub),
        None => PathBuf::from(PUBLISHED_ROOT),
    };
    dir.join(name).exists()
}

fn publish_download(src: &Path, subfolder: Option<&str>, name: &str) -> Result<PathBuf, String> {
    let res = super::smtc::with_bridge(|env, class| {
        let src_s = env.new_string(src.to_string_lossy())?;
        let sub_obj = match subfolder {
            Some(s) => JObject::from(env.new_string(s)?),
            None => JObject::null(),
        };
        let name_s = env.new_string(name)?;
        let out = env
            .call_static_method(
                class,
                "publishSoulseekDownload",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
                &[JValue::Object(&src_s), JValue::Object(&sub_obj), JValue::Object(&name_s)],
            )?
            .l()?;
        if out.is_null() {
            return Ok(Err("Could not publish the finished download to Music.".to_string()));
        }
        let text: String = env.get_string(&JString::from(out))?.into();
        if let Some(err) = text.strip_prefix("ERR:") {
            Ok(Err(err.to_string()))
        } else {
            Ok(Ok(PathBuf::from(text)))
        }
    });
    res.unwrap_or_else(|| Err("Could not publish the finished download to Music.".into()))
}

fn sync_allowed() {
    let allowed = APP_VISIBLE.load(Ordering::Acquire) || JOB_RUNNING.load(Ordering::Acquire);
    if let Some(slsk) = SOULSEEK.get() {
        slsk.set_allowed(allowed);
    }
}

#[no_mangle]
pub extern "system" fn Java_app_resonance_player_NativeBridge_setAppVisible(
    _env: JNIEnv,
    _class: JClass,
    visible: jboolean,
) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        APP_VISIBLE.store(visible != 0, Ordering::Release);
        sync_allowed();
    }));
}

#[no_mangle]
pub extern "system" fn Java_app_resonance_player_NativeBridge_setDownloadJobRunning(
    _env: JNIEnv,
    _class: JClass,
    running: jboolean,
) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        JOB_RUNNING.store(running != 0, Ordering::Release);
        sync_allowed();
    }));
}
