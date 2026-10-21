//! Tauri commands (the UI's API).

use std::path::Path;

use tauri::{AppHandle, Emitter, State};

use crate::audio::engine::{Command, PlayerStatus};
use crate::audio::eq::EqSettings;
use crate::audio::output::{self, DeviceInfo};
use crate::audio::types::{OutputMode, ReplayGainMode};
use crate::library::db::{self, Folder, Media, Playlist};
use crate::library::interchange::{self, ImportOutcome, PlaylistItem};
use crate::library::media::{self as media_files, Backdrop, MediaKind, Purpose};
use crate::library::{csv, m3u, xspf};
use crate::library::model::{is_audio_file, Track};
use crate::library::ScanRequest;
use crate::queue::{QueueSnapshot, RepeatMode};
use crate::settings::Settings;
use crate::AppState;

type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

pub fn emit_queue(app: &AppHandle, state: &AppState) {
    let snap = state.engine.queue.lock().snapshot();
    state.queue_dirty.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = app.emit("queue:changed", snap);
}

fn emit_playlists(app: &AppHandle) {
    let _ = app.emit("playlists:changed", ());
}

/// Expand dropped/opened paths: folders are walked recursively, files filtered to audio.
fn expand_paths(paths: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for p in paths {
        let path = Path::new(p);
        if path.is_dir() {
            let mut files: Vec<String> = walkdir::WalkDir::new(path)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file() && is_audio_file(e.path()))
                .map(|e| e.path().to_string_lossy().to_string())
                .collect();
            files.sort_by_key(|f| f.to_lowercase());
            out.extend(files);
        } else if path.is_file() && is_audio_file(path) {
            out.push(p.clone());
        }
    }
    out
}

fn start_queue(app: &AppHandle, state: &AppState, tracks: Vec<Track>, start: usize) {
    let item = state.engine.queue.lock().replace(tracks, start);
    if let Some(item) = item {
        state.engine.send(Command::PlayItem { uid: item.uid, start: 0.0, paused: false });
    }
    emit_queue(app, state);
}

/// Play files handed to the app from outside (command line / second instance).
pub fn open_external(app: &AppHandle, paths: Vec<String>) {
    use tauri::Manager;
    let app = app.clone();
    std::thread::spawn(move || {
        let Some(state) = app.try_state::<AppState>() else { return };
        let files = expand_paths(&paths);
        let tracks: Vec<Track> =
            state.library.resolve_paths(&files).into_iter().filter(|(_, m)| !m).map(|(t, _)| t).collect();
        if !tracks.is_empty() {
            start_queue(&app, &state, tracks, 0);
        }
    });
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

// ---- playback ---------------------------------------------------------------------------------

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> PlayerStatus {
    state.engine.status()
}

/// Play arbitrary files or folders (Explorer drag & drop, file picker, "Open with").
#[tauri::command]
pub async fn play_files(app: AppHandle, state: State<'_, AppState>, paths: Vec<String>) -> CmdResult<()> {
    let lib = state.library.clone();
    let tracks: Vec<Track> = tauri::async_runtime::spawn_blocking(move || {
        let files = expand_paths(&paths);
        lib.resolve_paths(&files).into_iter().filter(|(_, missing)| !missing).map(|(t, _)| t).collect()
    })
    .await
    .map_err(err)?;
    if tracks.is_empty() {
        return Err("No playable audio files were selected.".into());
    }
    start_queue(&app, &state, tracks, 0);
    Ok(())
}

/// Play files by path (playlists may reference files outside the library).
#[tauri::command]
pub async fn play_paths(app: AppHandle, state: State<'_, AppState>, paths: Vec<String>, start: usize) -> CmdResult<()> {
    let lib = state.library.clone();
    let start_path = paths.get(start).cloned();
    let tracks: Vec<Track> = tauri::async_runtime::spawn_blocking(move || {
        lib.resolve_paths(&paths).into_iter().filter(|(_, missing)| !missing).map(|(t, _)| t).collect()
    })
    .await
    .map_err(err)?;
    if tracks.is_empty() {
        return Err("None of these files can be found.".into());
    }
    let start = start_path.and_then(|p| tracks.iter().position(|t| t.path == p)).unwrap_or(0);
    start_queue(&app, &state, tracks, start);
    Ok(())
}

#[tauri::command]
pub fn toggle_play(state: State<'_, AppState>) {
    state.engine.send(Command::Toggle);
}

#[tauri::command]
pub fn play(state: State<'_, AppState>) {
    state.engine.send(Command::Play);
}

#[tauri::command]
pub fn pause(state: State<'_, AppState>) {
    state.engine.send(Command::Pause);
}

#[tauri::command]
pub fn stop(state: State<'_, AppState>) {
    state.engine.send(Command::Stop);
}

#[tauri::command]
pub fn next_track(state: State<'_, AppState>) {
    state.engine.send(Command::Next);
}

#[tauri::command]
pub fn previous_track(state: State<'_, AppState>) {
    state.engine.send(Command::Previous);
}

#[tauri::command]
pub fn seek(state: State<'_, AppState>, position: f64) {
    state.engine.send(Command::Seek(position));
}

#[tauri::command]
pub fn set_volume(state: State<'_, AppState>, volume: f64) {
    state.engine.set_volume(volume);
    state.settings.update(|s| s.volume = volume.clamp(0.0, 1.0));
}

#[tauri::command]
pub fn set_muted(state: State<'_, AppState>, muted: bool) {
    state.engine.set_muted(muted);
}

// ---- library ----------------------------------------------------------------------------------

#[tauri::command]
pub fn get_library_folders(state: State<'_, AppState>) -> CmdResult<Vec<Folder>> {
    state.library.folders().map_err(err)
}

#[tauri::command]
pub fn add_library_folder(state: State<'_, AppState>, path: String) -> CmdResult<Vec<Folder>> {
    let p = Path::new(&path);
    if !p.is_dir() {
        return Err(format!("\"{path}\" is not a folder."));
    }
    // Avoid overlapping roots: a folder inside an existing root would be scanned twice.
    let existing = state.library.folders().map_err(err)?;
    if let Some(parent) = existing.iter().find(|f| p.starts_with(&f.path)) {
        return Err(format!("\"{path}\" is already part of the library (inside \"{}\").", parent.path));
    }
    state.library.add_folder(&path).map_err(err)?;
    state.library.set_watching(state.settings.get().watch_folders);
    state.library.folders().map_err(err)
}

#[tauri::command]
pub fn remove_library_folder(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<Vec<Folder>> {
    state.library.remove_folder(id).map_err(err)?;
    state.library.set_watching(state.settings.get().watch_folders);
    let _ = app.emit("library:changed", crate::library::scan::ScanSummary::default());
    state.library.folders().map_err(err)
}

#[tauri::command]
pub fn rescan_library(state: State<'_, AppState>, force: bool) {
    state.library.request_scan(ScanRequest::All { force });
}

#[tauri::command]
pub fn is_scanning(state: State<'_, AppState>) -> bool {
    state.library.is_scanning()
}

#[tauri::command]
pub async fn get_tracks(state: State<'_, AppState>) -> CmdResult<Vec<Track>> {
    let lib = state.library.clone();
    tauri::async_runtime::spawn_blocking(move || lib.tracks())
        .await
        .map_err(err)?
        .map_err(err)
}

/// Replace the queue with library tracks (in the given order) and start at `start`.
#[tauri::command]
pub fn play_tracks(app: AppHandle, state: State<'_, AppState>, ids: Vec<i64>, start: usize) -> CmdResult<()> {
    let tracks = state.library.tracks_by_ids(&ids).map_err(err)?;
    if tracks.is_empty() {
        return Err("Those tracks are no longer in the library.".into());
    }
    let item = state.engine.queue.lock().replace(tracks, start);
    if let Some(item) = item {
        state.engine.send(Command::PlayItem { uid: item.uid, start: 0.0, paused: false });
    }
    emit_queue(&app, &state);
    Ok(())
}

/// Add library tracks to the queue: after the current track (`next`) or at the end.
#[tauri::command]
pub fn enqueue_tracks(app: AppHandle, state: State<'_, AppState>, ids: Vec<i64>, next: bool) -> CmdResult<usize> {
    let tracks = state.library.tracks_by_ids(&ids).map_err(err)?;
    let n = tracks.len();
    let start_now = {
        let mut q = state.engine.queue.lock();
        let was_empty = q.current().is_none();
        let items = if next { q.insert_next(tracks) } else { q.append(tracks) };
        if was_empty {
            items.first().map(|i| i.uid)
        } else {
            None
        }
    };
    match start_now {
        Some(uid) => state.engine.send(Command::PlayItem { uid, start: 0.0, paused: false }),
        None => state.engine.send(Command::QueueChanged),
    }
    emit_queue(&app, &state);
    Ok(n)
}

// ---- output -----------------------------------------------------------------------------------

#[tauri::command]
pub async fn list_devices() -> CmdResult<Vec<DeviceInfo>> {
    tauri::async_runtime::spawn_blocking(output::list_output_devices)
        .await
        .map_err(err)?
        .map_err(err)
}

#[tauri::command]
pub fn set_output(app: AppHandle, state: State<'_, AppState>, mode: OutputMode, device_id: Option<String>) -> Settings {
    state.engine.send(Command::SetOutput { mode, device_id: device_id.clone() });
    let s = state.settings.update(|s| {
        s.output_mode = mode;
        s.device_id = device_id;
    });
    let _ = app.emit("settings:changed", &s);
    s
}

// ---- queue ------------------------------------------------------------------------------------

#[tauri::command]
pub fn get_queue(state: State<'_, AppState>) -> QueueSnapshot {
    state.engine.queue.lock().snapshot()
}

#[tauri::command]
pub fn play_queue_item(state: State<'_, AppState>, uid: u64) {
    state.engine.send(Command::PlayItem { uid, start: 0.0, paused: false });
}

#[tauri::command]
pub fn remove_from_queue(app: AppHandle, state: State<'_, AppState>, uids: Vec<u64>) {
    state.engine.queue.lock().remove(&uids);
    state.engine.send(Command::QueueChanged);
    emit_queue(&app, &state);
}

#[tauri::command]
pub fn move_queue_item(app: AppHandle, state: State<'_, AppState>, from: usize, to: usize) {
    state.engine.queue.lock().move_item(from, to);
    state.engine.send(Command::QueueChanged);
    emit_queue(&app, &state);
}

#[tauri::command]
pub fn clear_queue(app: AppHandle, state: State<'_, AppState>) {
    state.engine.queue.lock().clear();
    state.engine.send(Command::QueueChanged);
    emit_queue(&app, &state);
}

#[tauri::command]
pub fn set_shuffle(app: AppHandle, state: State<'_, AppState>, shuffle: bool) {
    state.engine.queue.lock().set_shuffle(shuffle);
    state.engine.send(Command::QueueChanged);
    emit_queue(&app, &state);
}

#[tauri::command]
pub fn set_repeat(app: AppHandle, state: State<'_, AppState>, repeat: RepeatMode) {
    state.engine.queue.lock().repeat = repeat;
    state.engine.send(Command::QueueChanged);
    emit_queue(&app, &state);
}

// ---- settings ---------------------------------------------------------------------------------

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.get()
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    pub theme: Option<String>,
    pub close_to_tray: Option<bool>,
    pub watch_folders: Option<bool>,
    pub minimize_to_mini: Option<bool>,
    pub replay_gain: Option<ReplayGainMode>,
    pub replay_gain_preamp: Option<f64>,
    pub appearance: Option<crate::settings::Appearance>,
}

#[tauri::command]
pub fn update_settings(app: AppHandle, state: State<'_, AppState>, patch: SettingsPatch) -> Settings {
    let rg_changed = patch.replay_gain.is_some() || patch.replay_gain_preamp.is_some();
    let appearance = patch.appearance.map(|a| {
        let mut a = a.normalized();
        if a.background.as_ref().is_some_and(|b| !media_exists(&state, &b.media_id, false)) {
            a.background = None;
        }
        a
    });
    let s = state.settings.update(|s| {
        if let Some(v) = patch.theme {
            s.theme = v;
        }
        if let Some(v) = patch.close_to_tray {
            s.close_to_tray = v;
        }
        if let Some(v) = patch.watch_folders {
            s.watch_folders = v;
        }
        if let Some(v) = patch.minimize_to_mini {
            s.minimize_to_mini = v;
        }
        if let Some(v) = patch.replay_gain {
            s.replay_gain = v;
        }
        if let Some(v) = patch.replay_gain_preamp {
            s.replay_gain_preamp = v.clamp(-15.0, 15.0);
        }
        if let Some(v) = appearance {
            s.appearance = v;
        }
    });
    if rg_changed {
        state.engine.send(Command::SetReplayGain { mode: s.replay_gain, preamp_db: s.replay_gain_preamp });
    }
    let _ = app.emit("settings:changed", &s);
    s
}

// ---- playlists --------------------------------------------------------------------------------

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistEntry {
    pub track: Track,
    pub missing: bool,
    /// The imported reference this entry came from, if any (for review and Soulseek search).
    pub ref_key: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistDetail {
    pub playlist: Playlist,
    pub entries: Vec<PlaylistEntry>,
}

#[tauri::command]
pub fn get_playlists(state: State<'_, AppState>) -> CmdResult<Vec<Playlist>> {
    db::playlists(&state.library.db.lock()).map_err(err)
}

#[tauri::command]
pub fn create_playlist(app: AppHandle, state: State<'_, AppState>, name: String, ids: Vec<i64>) -> CmdResult<i64> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Playlist name cannot be empty.".into());
    }
    let paths: Vec<String> = state.library.tracks_by_ids(&ids).map_err(err)?.into_iter().map(|t| t.path).collect();
    let id = {
        let mut conn = state.library.db.lock();
        let id = db::create_playlist(&conn, name).map_err(err)?;
        db::set_playlist_paths(&mut conn, id, &paths).map_err(err)?;
        id
    };
    emit_playlists(&app);
    Ok(id)
}

#[tauri::command]
pub fn rename_playlist(app: AppHandle, state: State<'_, AppState>, id: i64, name: String) -> CmdResult<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Playlist name cannot be empty.".into());
    }
    db::rename_playlist(&state.library.db.lock(), id, name).map_err(err)?;
    emit_playlists(&app);
    Ok(())
}

#[tauri::command]
pub fn delete_playlist(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    db::delete_playlist(&state.library.db.lock(), id).map_err(err)?;
    emit_playlists(&app);
    Ok(())
}

#[tauri::command]
pub async fn get_playlist(state: State<'_, AppState>, id: i64) -> CmdResult<PlaylistDetail> {
    let lib = state.library.clone();
    tauri::async_runtime::spawn_blocking(move || -> CmdResult<PlaylistDetail> {
        let (playlist, items) = {
            let conn = lib.db.lock();
            let p = db::playlists(&conn)
                .map_err(err)?
                .into_iter()
                .find(|p| p.id == id)
                .ok_or_else(|| "Playlist not found.".to_string())?;
            (p, db::playlist_items(&conn, id).map_err(err)?)
        };
        let paths: Vec<String> = items.iter().map(|(p, _)| p.clone()).collect();
        let resolved = lib.resolve_paths(&paths);
        let conn = lib.db.lock();
        let entries = resolved
            .into_iter()
            .zip(items)
            .map(|((mut track, missing), (_, ref_key))| {
                // A song you don't have: show what the source called it, not a file name.
                if missing {
                    if let Some(r) = ref_key
                        .as_deref()
                        .and_then(|k| db::match_ref_json(&conn, k).ok().flatten())
                        .and_then(|j| serde_json::from_str::<interchange::TrackRef>(&j).ok())
                    {
                        track.title = if r.title.is_empty() { track.title } else { r.title };
                        track.artist = r.artists.join(", ");
                        track.album = r.album;
                        track.duration = r.duration_ms.map(|ms| ms as f64 / 1000.0).unwrap_or(0.0);
                    }
                }
                PlaylistEntry { track, missing, ref_key }
            })
            .collect();
        Ok(PlaylistDetail { playlist, entries })
    })
    .await
    .map_err(err)?
}

/// Append library tracks to a playlist.
#[tauri::command]
pub fn add_to_playlist(app: AppHandle, state: State<'_, AppState>, id: i64, ids: Vec<i64>) -> CmdResult<usize> {
    let add: Vec<String> = state.library.tracks_by_ids(&ids).map_err(err)?.into_iter().map(|t| t.path).collect();
    let n = add.len();
    {
        let mut conn = state.library.db.lock();
        let mut paths = db::playlist_paths(&conn, id).map_err(err)?;
        paths.extend(add);
        db::set_playlist_paths(&mut conn, id, &paths).map_err(err)?;
    }
    emit_playlists(&app);
    Ok(n)
}

/// Replace a playlist's contents (used for reordering and removing entries).
#[tauri::command]
pub fn set_playlist_paths(app: AppHandle, state: State<'_, AppState>, id: i64, paths: Vec<String>) -> CmdResult<()> {
    db::set_playlist_paths(&mut state.library.db.lock(), id, &paths).map_err(err)?;
    emit_playlists(&app);
    Ok(())
}

/// Import an M3U/M3U8, PLS, XSPF or WPL file. Importing the same file again updates its playlist.
#[tauri::command]
pub async fn import_playlist(app: AppHandle, state: State<'_, AppState>, path: String) -> CmdResult<ImportOutcome> {
    let lib = state.library.clone();
    let res = tauri::async_runtime::spawn_blocking(move || -> CmdResult<ImportOutcome> {
        let file = Path::new(&path);
        let pl = interchange::parse_file(file).map_err(|e| format!("{e:#}"))?;
        let base = file.parent().unwrap_or(Path::new("."));
        interchange::import_into_db(&mut lib.db.lock(), &pl, base).map_err(err)
    })
    .await
    .map_err(err)??;
    emit_playlists(&app);
    Ok(res)
}

/// Export a playlist as XSPF or CSV (by extension) or extended M3U8 (anything else).
#[tauri::command]
pub async fn export_playlist(state: State<'_, AppState>, id: i64, path: String) -> CmdResult<usize> {
    let lib = state.library.clone();
    tauri::async_runtime::spawn_blocking(move || -> CmdResult<usize> {
        let (name, paths) = {
            let conn = lib.db.lock();
            let name = db::playlists(&conn).map_err(err)?.into_iter().find(|p| p.id == id).map(|p| p.name).unwrap_or_default();
            (name, db::playlist_paths(&conn, id).map_err(err)?)
        };
        let items: Vec<PlaylistItem> = lib
            .resolve_paths(&paths)
            .into_iter()
            .map(|(t, _)| PlaylistItem { track: interchange::track_ref_from_track(&t), added_at: None })
            .collect();
        let n = items.len();
        let pl = interchange::Playlist { name, source: "resonance".into(), source_id: None, items };
        let file = Path::new(&path);
        let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        match ext.as_str() {
            "xspf" => xspf::write_file(file, &pl),
            "csv" => csv::write_file(file, &pl),
            _ => m3u::write_file(file, &pl),
        }
        .map_err(err)?;
        Ok(n)
    })
    .await
    .map_err(err)?
}

/// The review screen for a playlist: imported entries waiting for a decision (with ranked
/// candidates and reasons), entries decided earlier (for undo), and counts of the rest.
#[tauri::command]
pub async fn get_playlist_review(state: State<'_, AppState>, id: i64) -> CmdResult<interchange::PlaylistReview> {
    let lib = state.library.clone();
    tauri::async_runtime::spawn_blocking(move || interchange::playlist_review(&mut lib.db.lock(), id).map_err(err))
        .await
        .map_err(err)?
}

/// Decide for an imported reference: "accept" a file (`path`), "missing", "skip", or "clear" to
/// undo. Applies to every playlist that has the reference; decisions are never asked again.
#[tauri::command]
pub async fn decide_match(app: AppHandle, state: State<'_, AppState>, key: String, decision: String, path: Option<String>) -> CmdResult<usize> {
    let lib = state.library.clone();
    let n = tauri::async_runtime::spawn_blocking(move || {
        interchange::decide(&mut lib.db.lock(), &key, &decision, path.as_deref()).map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(err)??;
    emit_playlists(&app);
    Ok(n)
}

/// Write a CSV of a playlist's imported entries that aren't matched to a local file, with the
/// reason and the best candidate. Returns how many rows were written.
#[tauri::command]
pub async fn export_unmatched_report(state: State<'_, AppState>, id: i64, path: String) -> CmdResult<usize> {
    let lib = state.library.clone();
    tauri::async_runtime::spawn_blocking(move || -> CmdResult<usize> {
        let rows = interchange::rematch_playlist(&mut lib.db.lock(), id).map_err(err)?;
        let n = rows.iter().filter(|(_, _, m)| m.accepted.is_none()).count();
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(crate::library::matcher::unmatched_report(&rows).as_bytes());
        std::fs::write(&path, bytes).map_err(|e| format!("Cannot write {path}: {e}"))?;
        Ok(n)
    })
    .await
    .map_err(err)?
}

/// Save the current queue as a new playlist.
#[tauri::command]
pub fn save_queue_as_playlist(app: AppHandle, state: State<'_, AppState>, name: String) -> CmdResult<i64> {
    let paths: Vec<String> = state.engine.queue.lock().snapshot().items.into_iter().map(|i| i.track.path).collect();
    let id = {
        let mut conn = state.library.db.lock();
        let id = db::create_playlist(&conn, name.trim()).map_err(err)?;
        db::set_playlist_paths(&mut conn, id, &paths).map_err(err)?;
        id
    };
    emit_playlists(&app);
    Ok(id)
}

// ---- equalizer --------------------------------------------------------------------------------

#[derive(serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EqPreset {
    pub name: String,
    pub eq: EqSettings,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoEqImport {
    pub eq: EqSettings,
    pub name: String,
    pub warnings: Vec<String>,
}

fn presets_file(app: &AppHandle) -> CmdResult<std::path::PathBuf> {
    use tauri::Manager;
    Ok(app.path().app_config_dir().map_err(err)?.join("eq_presets.json"))
}

pub(crate) fn read_presets(app: &AppHandle) -> CmdResult<Vec<EqPreset>> {
    let f = presets_file(app)?;
    Ok(std::fs::read(&f).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default())
}

pub(crate) fn write_presets(app: &AppHandle, presets: &[EqPreset]) -> CmdResult<()> {
    let f = presets_file(app)?;
    let tmp = f.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(presets).map_err(err)?).map_err(err)?;
    std::fs::rename(&tmp, &f).map_err(err)
}

/// Apply EQ settings live and persist them.
#[tauri::command]
pub fn set_eq(state: State<'_, AppState>, eq: EqSettings) -> EqSettings {
    let eq = eq.normalized();
    state.engine.set_eq(eq.clone());
    state.settings.update(|s| s.eq = eq.clone());
    eq
}

#[tauri::command]
pub fn get_eq_presets(app: AppHandle) -> CmdResult<Vec<EqPreset>> {
    read_presets(&app)
}

#[tauri::command]
pub fn save_eq_preset(app: AppHandle, name: String, eq: EqSettings) -> CmdResult<Vec<EqPreset>> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Preset name cannot be empty.".into());
    }
    let mut presets = read_presets(&app)?;
    let mut eq = eq.normalized();
    eq.preset_name = Some(name.clone());
    match presets.iter_mut().find(|p| p.name.eq_ignore_ascii_case(&name)) {
        Some(p) => p.eq = eq,
        None => presets.push(EqPreset { name, eq }),
    }
    presets.sort_by_key(|p| p.name.to_lowercase());
    write_presets(&app, &presets)?;
    Ok(presets)
}

#[tauri::command]
pub fn delete_eq_preset(app: AppHandle, name: String) -> CmdResult<Vec<EqPreset>> {
    let mut presets = read_presets(&app)?;
    presets.retain(|p| p.name != name);
    write_presets(&app, &presets)?;
    Ok(presets)
}

/// Parse an AutoEQ ParametricEQ.txt (Equalizer APO format). Does not apply it.
#[tauri::command]
pub fn import_autoeq(path: String) -> CmdResult<AutoEqImport> {
    let p = Path::new(&path);
    let text = std::fs::read_to_string(p).map_err(|e| format!("Cannot read {path}: {e}"))?;
    let (mut eq, warnings) = crate::audio::eq::parse_autoeq(&text)?;
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    // "Sennheiser HD 600 ParametricEQ" -> "Sennheiser HD 600"
    let name = stem.replace("ParametricEQ", "").replace("Parametric EQ", "").trim().trim_end_matches(['-', '_']).trim().to_string();
    let name = if name.is_empty() { "AutoEQ".to_string() } else { name };
    eq.preset_name = Some(name.clone());
    Ok(AutoEqImport { eq, name, warnings })
}

/// Open an https:// link (e.g. the donate page) in the default browser.
#[tauri::command]
pub fn open_url(url: String) -> CmdResult<()> {
    let ok = url.starts_with("https://") && !url.chars().any(|c| c.is_whitespace() || c == '"' || c.is_control());
    if !ok {
        return Err("Only https:// links can be opened.".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", &url])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("Cannot open {url}: {e}"))?;
    }
    Ok(())
}

// ---- customization ----------------------------------------------------------------------------

#[tauri::command]
pub fn update_playlist(app: AppHandle, state: State<'_, AppState>, id: i64, name: String, description: String) -> CmdResult<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Playlist name cannot be empty.".into());
    }
    db::update_playlist(&state.library.db.lock(), id, name, description.trim()).map_err(err)?;
    emit_playlists(&app);
    Ok(())
}

fn emit_media(app: &AppHandle) {
    let _ = app.emit("media:changed", ());
}

/// Whether `id` is a stored media file (with `animated`, a GIF or video rather than a photo).
fn media_exists(state: &AppState, id: &str, animated: bool) -> bool {
    let row = db::media_by_id(&state.library.db.lock(), id).ok().flatten();
    row.is_some_and(|m| !animated || m.kind.animated()) && state.library.media.file(id).is_some()
}

/// What `stage_media` made of a file.
#[derive(serde::Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Staged {
    /// A still picture for a cover, stored in the artwork cache like any other cover.
    Cover { cover_id: String },
    /// A still picture for a background, stored and ready to use.
    Ready { media: Media },
    /// A GIF or video copied into the app, waiting for `commit_media` with its size, length and
    /// poster (the frontend reads those from the file).
    Pending { id: String, kind: MediaKind, mime: String, bytes: u64 },
}

/// Check a photo, GIF or video for `purpose` and copy it into the app's cache (the original file
/// isn't needed afterwards). Refusals come back as plain messages.
#[tauri::command]
pub async fn stage_media(app: AppHandle, state: State<'_, AppState>, path: String, purpose: Purpose) -> CmdResult<Staged> {
    let lib = state.library.clone();
    let staged = tauri::async_runtime::spawn_blocking(move || -> CmdResult<Staged> {
        let checked = media_files::read_checked(Path::new(&path), purpose)?;
        let unreadable = || "That picture can't be read.".to_string();
        if checked.format.kind() != MediaKind::Image {
            let id = lib.media.put(&checked.data, checked.format.ext()).map_err(err)?;
            return Ok(Staged::Pending {
                id,
                kind: checked.format.kind(),
                mime: checked.format.mime().into(),
                bytes: checked.data.len() as u64,
            });
        }
        if purpose == Purpose::Cover {
            return lib.covers.store(&checked.data).map(|cover_id| Staged::Cover { cover_id }).ok_or_else(unreadable);
        }
        let (data, format, width, height) = media_files::fit_still_image(checked.data, checked.format)?;
        let poster_id = lib.covers.store(&data).ok_or_else(unreadable)?;
        let id = lib.media.put(&data, format.ext()).map_err(err)?;
        let media = Media {
            id,
            kind: MediaKind::Image,
            mime: format.mime().into(),
            width,
            height,
            duration_ms: None,
            bytes: data.len() as u64,
            poster_id: Some(poster_id),
        };
        db::put_media(&lib.db.lock(), &media).map_err(err)?;
        Ok(Staged::Ready { media })
    })
    .await
    .map_err(err)??;
    if matches!(staged, Staged::Ready { .. }) {
        emit_media(&app);
    }
    Ok(staged)
}

/// Finish importing a GIF or video from `stage_media`: `width`, `height` and `duration_ms` were
/// read from the file by the frontend, `poster` is its first frame (base64 JPEG or PNG).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn commit_media(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    purpose: Purpose,
    width: u32,
    height: u32,
    duration_ms: Option<u64>,
    poster: String,
) -> CmdResult<Media> {
    use base64::Engine as _;
    let lib = state.library.clone();
    let media = tauri::async_runtime::spawn_blocking(move || -> CmdResult<Media> {
        let gone = || "That file is no longer available. Choose it again.".to_string();
        let (path, mime) = lib.media.file(&id).ok_or_else(gone)?;
        let format = match mime {
            "video/mp4" => media_files::Format::Mp4,
            "video/webm" => media_files::Format::WebM,
            "image/gif" => media_files::Format::Gif { animated: true },
            "image/webp" => media_files::Format::Webp { animated: true },
            _ => return Err(gone()),
        };
        let bytes = std::fs::metadata(&path).map_err(|_| gone())?.len();
        media_files::check_size(format, purpose, bytes)?;
        if format.kind() == MediaKind::Video {
            media_files::check_video(purpose, width, height, duration_ms.unwrap_or(0))?;
        }
        let frame = base64::engine::general_purpose::STANDARD
            .decode(poster.trim_start_matches("data:image/jpeg;base64,").trim_start_matches("data:image/png;base64,"))
            .map_err(err)?;
        let poster_id = lib.covers.store(&frame).ok_or_else(|| "Couldn't make a still frame of that file.".to_string())?;
        let media = Media {
            id,
            kind: format.kind(),
            mime: mime.into(),
            width,
            height,
            duration_ms: duration_ms.filter(|_| format.kind() == MediaKind::Video),
            bytes,
            poster_id: Some(poster_id),
        };
        db::put_media(&lib.db.lock(), &media).map_err(err)?;
        Ok(media)
    })
    .await
    .map_err(err)??;
    emit_media(&app);
    Ok(media)
}

/// Drop a staged file that the frontend refused (too long, too large…). Files already in use stay.
#[tauri::command]
pub fn discard_media(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    if db::media_by_id(&state.library.db.lock(), &id).map_err(err)?.is_none() {
        state.library.media.remove(&id);
    }
    Ok(())
}

/// Lyrics for a song, from a `.lrc` file next to it or its tags (never from the network).
/// `None` when it has none.
#[tauri::command]
pub async fn get_lyrics(path: String) -> CmdResult<Option<crate::library::lyrics::Lyrics>> {
    tauri::async_runtime::spawn_blocking(move || {
        let p = Path::new(&path);
        is_audio_file(p).then(|| crate::library::lyrics::read(p)).flatten()
    })
    .await
    .map_err(err)
}

/// Every stored photo, GIF and video, with its kind, size and poster.
#[tauri::command]
pub fn get_media(state: State<'_, AppState>) -> CmdResult<Vec<Media>> {
    db::media(&state.library.db.lock()).map_err(err)
}

/// Set (or with `None`, remove) a playlist's photo. `motion_id` is a GIF or video from
/// `commit_media`; `cover_id` is then its poster.
#[tauri::command]
pub fn set_playlist_cover(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    cover_id: Option<String>,
    motion_id: Option<String>,
) -> CmdResult<()> {
    if let Some(c) = &cover_id {
        if state.library.covers.file(c, false).is_none() {
            return Err("That image is no longer available.".into());
        }
    }
    if motion_id.as_deref().is_some_and(|m| !media_exists(&state, m, true)) {
        return Err("That GIF or video is no longer available.".into());
    }
    db::set_playlist_cover(&state.library.db.lock(), id, cover_id.as_deref(), motion_id.as_deref()).map_err(err)?;
    emit_playlists(&app);
    Ok(())
}

/// Set (or with `None`, remove) the photo, GIF or video behind a playlist's page.
#[tauri::command]
pub fn set_playlist_background(app: AppHandle, state: State<'_, AppState>, id: i64, background: Option<Backdrop>) -> CmdResult<()> {
    let background = match background {
        Some(b) => {
            let b = b.normalized().ok_or_else(|| "That background is no longer available.".to_string())?;
            if !media_exists(&state, &b.media_id, false) {
                return Err("That background is no longer available.".into());
            }
            Some(b)
        }
        None => None,
    };
    db::set_playlist_background(&state.library.db.lock(), id, background.as_ref()).map_err(err)?;
    emit_playlists(&app);
    Ok(())
}

/// Hide or restore artwork for tracks (in the app only; music files are untouched).
#[tauri::command]
pub fn set_tracks_cover_hidden(app: AppHandle, state: State<'_, AppState>, ids: Vec<i64>, hidden: bool) -> CmdResult<()> {
    db::set_cover_hidden(&mut state.library.db.lock(), &ids, hidden).map_err(err)?;
    let _ = app.emit("library:changed", crate::library::scan::ScanSummary::default());
    emit_playlists(&app);
    Ok(())
}

// ---- backup -----------------------------------------------------------------------------------

/// Write settings, EQ presets, library folders, playlists and artwork choices to `path`.
/// `ui` holds frontend preferences to carry along.
#[tauri::command]
pub async fn export_backup(app: AppHandle, state: State<'_, AppState>, path: String, ui: serde_json::Value) -> CmdResult<crate::backup::ExportSummary> {
    let lib = state.library.clone();
    let settings = state.settings.get();
    let presets = read_presets(&app)?;
    tauri::async_runtime::spawn_blocking(move || crate::backup::export(&lib, settings, presets, ui, Path::new(&path)))
        .await
        .map_err(err)?
}

/// Restore a backup made by `export_backup`, merging it into this PC's library.
#[tauri::command]
pub async fn import_backup(app: AppHandle, state: State<'_, AppState>, path: String) -> CmdResult<crate::backup::RestoreSummary> {
    let lib = state.library.clone();
    let restored = tauri::async_runtime::spawn_blocking(move || -> CmdResult<crate::backup::Restored> {
        let b = crate::backup::read(Path::new(&path))?;
        let adder = lib.clone();
        crate::backup::restore(&lib, b, |f| adder.add_folder(f).map(|_| ()).map_err(err))
    })
    .await
    .map_err(err)??;

    // EQ presets: the backup's version wins on name clashes.
    let mut presets = read_presets(&app)?;
    for p in restored.eq_presets {
        presets.retain(|e| !e.name.eq_ignore_ascii_case(&p.name));
        presets.push(p);
    }
    presets.sort_by_key(|p| p.name.to_lowercase());
    write_presets(&app, &presets)?;

    // Settings, except the audio device and output mode (those belong to this PC's hardware).
    let mut summary = restored.summary;
    let from = summary.settings.clone();
    let s = state.settings.update(|s| {
        s.volume = from.volume.clamp(0.0, 1.0);
        s.replay_gain = from.replay_gain;
        s.replay_gain_preamp = from.replay_gain_preamp;
        s.theme = from.theme.clone();
        s.close_to_tray = from.close_to_tray;
        s.watch_folders = from.watch_folders;
        s.eq = from.eq.clone().normalized();
        s.appearance = from.appearance.clone().normalized();
    });
    state.engine.set_volume(s.volume);
    state.engine.set_eq(s.eq.clone());
    state.engine.send(Command::SetReplayGain { mode: s.replay_gain, preamp_db: s.replay_gain_preamp });
    state.library.set_watching(s.watch_folders);
    let _ = app.emit("settings:changed", &s);
    let _ = app.emit("library:changed", crate::library::scan::ScanSummary::default());
    emit_playlists(&app);
    summary.settings = s;
    Ok(summary)
}

/// Give tracks custom artwork (a cover id from `stage_media`, or a GIF/video's poster with the
/// GIF/video as `motion_id`), or with `None` go back to the artwork from their files. Stored in the
/// app only; music files are untouched.
#[tauri::command]
pub fn set_tracks_cover(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<i64>,
    cover_id: Option<String>,
    motion_id: Option<String>,
) -> CmdResult<()> {
    if let Some(c) = &cover_id {
        if state.library.covers.file(c, false).is_none() {
            return Err("That image is no longer available.".into());
        }
    }
    if motion_id.as_deref().is_some_and(|m| !media_exists(&state, m, true)) {
        return Err("That GIF or video is no longer available.".into());
    }
    db::set_custom_cover(&mut state.library.db.lock(), &ids, cover_id.as_deref(), motion_id.as_deref()).map_err(err)?;
    let _ = app.emit("library:changed", crate::library::scan::ScanSummary::default());
    emit_playlists(&app);
    Ok(())
}
