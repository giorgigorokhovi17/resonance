//! Phone sync serves the desktop library to a phone; on the phone itself there is nothing to
//! serve. Same API as the desktop `phone.rs`, never running.

use serde::Serialize;
use tauri::AppHandle;

use crate::settings::PhoneSyncSettings;

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct PhoneStatus {
    pub running: bool,
    pub url: Option<String>,
    pub qr: Option<String>,
    pub error: Option<String>,
}

pub fn apply(_app: &AppHandle, _s: &PhoneSyncSettings) -> PhoneStatus {
    status()
}

pub fn status() -> PhoneStatus {
    PhoneStatus { error: Some("Phone sync runs on the desktop app.".into()), ..Default::default() }
}
