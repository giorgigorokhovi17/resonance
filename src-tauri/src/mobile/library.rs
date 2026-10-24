//! Music on an Android phone (docs/android-design.md §4): the folder picker and rescans.
//!
//! The app reads audio by its real path with the READ_MEDIA_AUDIO permission, so the library,
//! the matcher and backups work exactly as on Windows. The system folder picker only chooses
//! which folders; MainActivity.kt turns the picked folder into its path.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crossbeam_channel::{bounded, Sender};
use jni::objects::{JByteArray, JClass, JString, JValue};
use jni::JNIEnv;

use crate::library::{Library, ScanRequest};

static LIBRARY: OnceLock<Arc<Library>> = OnceLock::new();
/// The picker that's open, waiting for MainActivity's answer.
static PICK: Mutex<Option<Sender<Result<Option<String>, String>>>> = Mutex::new(None);

pub fn start(library: Arc<Library>) {
    let _ = LIBRARY.set(library);
    crate::library::covers::set_folder_image_reader(folder_image);
}

/// Folder pictures beside the songs, which the audio permission hides: read through the folder
/// picker's grant (Storage.kt).
fn folder_image(dir: &Path) -> Option<Vec<u8>> {
    super::smtc::with_bridge(|env, class| {
        let dir = env.new_string(dir.to_string_lossy())?;
        let bytes = env
            .call_static_method(class, "folderImage", "(Ljava/lang/String;)[B", &[JValue::Object(&dir)])?
            .l()?;
        if bytes.is_null() {
            return Ok(None);
        }
        env.convert_byte_array(JByteArray::from(bytes)).map(Some)
    })
    .flatten()
}

/// Asks for the audio permission if needed, opens the system folder picker, and returns the
/// picked folder's path (None when cancelled).
pub fn pick_folder() -> Result<Option<String>, String> {
    let (tx, rx) = bounded(1);
    *PICK.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
    super::smtc::call_static("pickMusicFolder")?;
    // The user may take a while; give up only if the screen went away without an answer.
    rx.recv_timeout(Duration::from_secs(30 * 60)).unwrap_or(Ok(None))
}

fn rescan() {
    if let Some(library) = LIBRARY.get() {
        library.request_scan(ScanRequest::All { force: false });
    }
}

/// NativeBridge.folderPicked(path, error): the picker's answer.
#[no_mangle]
pub extern "system" fn Java_app_resonance_player_NativeBridge_folderPicked(
    mut env: JNIEnv,
    _class: JClass,
    path: JString,
    error: JString,
) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let text = |env: &mut JNIEnv, s: &JString| -> Option<String> {
            if s.is_null() {
                None
            } else {
                env.get_string(s).ok().map(String::from)
            }
        };
        let answer = match text(&mut env, &error) {
            Some(e) => Err(e),
            None => Ok(text(&mut env, &path)),
        };
        if let Some(tx) = PICK.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = tx.send(answer);
        }
    }));
}

/// NativeBridge.libraryChanged(): back on screen, or Android's media index changed. A quick
/// incremental scan picks up songs added or removed meanwhile.
#[no_mangle]
pub extern "system" fn Java_app_resonance_player_NativeBridge_libraryChanged(_env: JNIEnv, _class: JClass) {
    let _ = catch_unwind(rescan);
}
