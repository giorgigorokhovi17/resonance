//! File sync runs on the phone app (mobile/pc_sync.rs). Same API here, never running.

use tauri::AppHandle;
use undertone_core::pc_sync::Status;

pub fn status() -> Status {
    Status::default()
}

pub fn cancel() {}

pub fn start(_app: &AppHandle) -> Result<(), String> {
    Err("File sync runs on the phone app.".into())
}
