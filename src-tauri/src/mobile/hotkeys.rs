//! Global shortcuts don't exist on phones; headset and Bluetooth buttons come through the media
//! session instead. Same API as the desktop `hotkeys.rs`, doing nothing.

use serde::Serialize;
use tauri::AppHandle;

use crate::settings::HotkeySettings;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    pub action: String,
    pub keys: String,
    pub message: String,
}

pub fn apply(_app: &AppHandle, _s: &HotkeySettings) {}

pub fn suspend(_app: &AppHandle) {}

pub fn problems() -> Vec<Problem> {
    Vec::new()
}
