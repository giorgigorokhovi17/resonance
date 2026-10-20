//! Tauri commands for the Sources page.

use serde::Serialize;
use tauri::State;

use super::soulseek::{self, DownloadRequest, Job, Status};
use crate::settings::SoulseekSettings;
use crate::AppState;

type CmdResult<T> = Result<T, String>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlskSettingsView {
    #[serde(flatten)]
    pub settings: SoulseekSettings,
    /// The folder downloads actually go to (the setting, or the default), if there is one.
    pub effective_folder: Option<String>,
    /// Whether that folder is inside a library folder, so downloads show up in the library.
    pub in_library: bool,
}

fn library_folders(state: &AppState) -> Vec<String> {
    state.library.folders().map(|f| f.into_iter().map(|f| f.path).collect()).unwrap_or_default()
}

fn settings_view(state: &AppState) -> SlskSettingsView {
    let settings = state.settings.get().soulseek;
    let libs = library_folders(state);
    let folder = soulseek::download_folder(settings.download_folder.as_deref(), &libs);
    let in_library = folder.as_ref().is_some_and(|f| {
        let f = f.to_string_lossy().to_lowercase();
        libs.iter().any(|l| {
            let l = l.trim_end_matches('\\').to_lowercase();
            f == l || f.starts_with(&format!("{l}\\"))
        })
    });
    SlskSettingsView { settings, effective_folder: folder.map(|f| f.to_string_lossy().into_owned()), in_library }
}

#[tauri::command]
pub fn slsk_status(state: State<'_, AppState>) -> Status {
    state.soulseek.status()
}

#[tauri::command]
pub async fn slsk_sign_in(state: State<'_, AppState>, username: String, password: String, remember: bool) -> CmdResult<Status> {
    let slsk = state.soulseek.clone();
    let settings = state.settings.get().soulseek;
    tauri::async_runtime::spawn_blocking(move || slsk.sign_in(username, password, remember, settings))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn slsk_sign_out(state: State<'_, AppState>, forget: bool) {
    state.soulseek.sign_out(forget);
}

#[tauri::command]
pub fn slsk_search(state: State<'_, AppState>, query: String) -> CmdResult<u64> {
    state.soulseek.search(query)
}

#[tauri::command]
pub fn slsk_stop_search(state: State<'_, AppState>) {
    state.soulseek.stop_search();
}

#[tauri::command]
pub fn slsk_downloads(state: State<'_, AppState>) -> Vec<Job> {
    state.soulseek.jobs()
}

#[tauri::command]
pub fn slsk_download(state: State<'_, AppState>, items: Vec<DownloadRequest>) -> CmdResult<usize> {
    let folder = settings_view(&state)
        .effective_folder
        .ok_or("Choose a download folder first, or add a library folder in Settings.")?;
    state.soulseek.download(std::path::Path::new(&folder), items)
}

#[tauri::command]
pub fn slsk_cancel(state: State<'_, AppState>, id: u64) {
    state.soulseek.cancel(id);
}

#[tauri::command]
pub fn slsk_retry(state: State<'_, AppState>, id: u64) -> CmdResult<()> {
    state.soulseek.retry(id)
}

#[tauri::command]
pub fn slsk_clear_finished(state: State<'_, AppState>) {
    state.soulseek.clear_finished();
}

#[tauri::command]
pub fn slsk_settings(state: State<'_, AppState>) -> SlskSettingsView {
    settings_view(&state)
}

/// Saves Soulseek options. Listening changes apply at the next sign-in.
#[tauri::command]
pub fn slsk_update_settings(state: State<'_, AppState>, settings: SoulseekSettings) -> SlskSettingsView {
    state.settings.update(|s| {
        s.soulseek = SoulseekSettings {
            download_folder: settings.download_folder.map(|f| f.trim().to_string()).filter(|f| !f.is_empty()),
            listen: settings.listen,
            listen_port: if settings.listen_port < 1024 { 2234 } else { settings.listen_port },
        };
    });
    settings_view(&state)
}
