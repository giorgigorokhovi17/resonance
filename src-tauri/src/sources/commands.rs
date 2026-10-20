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

// ---- paste-a-link import ----------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkImport {
    #[serde(flatten)]
    pub outcome: crate::library::interchange::ImportOutcome,
    /// "Spotify", "YouTube" or "Apple Music".
    pub service: &'static str,
    /// Set when the service only gave part of the playlist.
    pub note: Option<String>,
}

/// Whether `url` is a link Undertone can import (for live feedback while typing).
#[tauri::command]
pub fn check_link(url: String) -> Option<super::links::LinkInfo> {
    super::links::parse_link(&url).map(|l| l.info())
}

/// Read a public Spotify / YouTube / Apple Music playlist or album link and import it: every
/// song goes through the matcher; ones you don't have stay in the playlist as missing, ready to
/// be found on Soulseek. Importing the same link again updates its playlist.
#[tauri::command]
pub async fn import_link(app: tauri::AppHandle, state: State<'_, AppState>, url: String) -> CmdResult<LinkImport> {
    use tauri::Emitter;
    let link = super::links::parse_link(&url)
        .ok_or("That isn't a public Spotify, YouTube or Apple Music playlist or album link.")?;
    let fetched = super::links::fetch(&link).await.map_err(|e| format!("{e:#}"))?;
    let lib = state.library.clone();
    let playlist = fetched.playlist;
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        crate::library::interchange::import_into_db(&mut lib.db.lock(), &playlist, std::path::Path::new("."))
            .map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| e.to_string())??;
    let _ = app.emit("playlists:changed", ());
    Ok(LinkImport { outcome, service: link.info().service, note: fetched.note })
}

// ---- missing songs → Soulseek -----------------------------------------------------------------

/// Search Soulseek for the songs an imported playlist is missing: entries with nothing like them
/// in the library, plus ones waiting for review (they may be a different version). `keys` limits
/// it to some entries. Results arrive as `slsk:find` events; nothing downloads until approved.
#[tauri::command]
pub async fn find_missing(state: State<'_, AppState>, playlist_id: i64, keys: Option<Vec<String>>) -> CmdResult<usize> {
    if state.soulseek.status().state != soulseek::ConnState::SignedIn {
        return Err("Sign in to Soulseek first (Sources page).".into());
    }
    let lib = state.library.clone();
    let review = tauri::async_runtime::spawn_blocking(move || {
        crate::library::interchange::playlist_review(&mut lib.db.lock(), playlist_id).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    let refs: Vec<(String, crate::library::interchange::TrackRef)> = review
        .missing
        .into_iter()
        .chain(review.pending)
        .filter(|i| keys.as_ref().is_none_or(|k| k.contains(&i.result.ref_key)))
        .map(|i| (i.result.ref_key, i.reference))
        .collect();
    if refs.is_empty() {
        return Err("Nothing in this playlist is missing.".into());
    }
    Ok(state.finder.start(state.soulseek.clone(), playlist_id, refs))
}

#[tauri::command]
pub fn stop_find(state: State<'_, AppState>, playlist_id: i64) {
    state.finder.stop(playlist_id);
}

/// Cancel the Soulseek search for one song (skipped if waiting, stopped if running).
#[tauri::command]
pub fn skip_find(state: State<'_, AppState>, key: String) {
    state.finder.skip(&key);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindStatus {
    pub running: bool,
    pub results: Vec<super::missing::FindResult>,
}

/// Results found so far for these entries (kept while the app runs).
#[tauri::command]
pub fn find_results(state: State<'_, AppState>, playlist_id: i64, keys: Vec<String>) -> FindStatus {
    FindStatus { running: state.finder.is_running(playlist_id), results: state.finder.results(&keys) }
}

/// Download approved files; each one replaces its playlist entry when it finishes.
#[tauri::command]
pub fn download_found(state: State<'_, AppState>, items: Vec<DownloadRequest>) -> CmdResult<usize> {
    if items.iter().any(|i| i.link_key.is_none()) {
        return Err("Each file needs the playlist entry it's for.".into());
    }
    let folder = settings_view(&state)
        .effective_folder
        .ok_or("Choose a download folder first, or add a library folder in Settings.")?;
    state.soulseek.download(std::path::Path::new(&folder), items)
}
