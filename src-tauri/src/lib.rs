pub mod audio;
pub mod backup;
pub mod commands;
pub mod hotkeys;
pub mod phone;
pub mod island;
pub mod library;
pub mod persist;
pub mod queue;
pub mod settings;
pub mod sleep;
pub mod smtc;
pub mod sources;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crossbeam_channel::Sender;
use parking_lot::Mutex;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};

use audio::engine::{Command, Engine, EngineConfig, EngineEvent};
use audio::types::PlayState;
use library::{Library, LibraryEvent};
use settings::SettingsStore;
use smtc::SmtcUpdate;

pub struct AppState {
    pub engine: Arc<Engine>,
    pub settings: SettingsStore,
    pub library: Arc<Library>,
    pub queue_dirty: Arc<AtomicBool>,
    pub queue_file: PathBuf,
    pub soulseek: Arc<sources::soulseek::Soulseek>,
    /// Finds songs missing from imported playlists on Soulseek.
    pub finder: Arc<sources::missing::Finder>,
    /// "Other versions" searches for one missing song.
    pub versions: Arc<sources::versions::Versions>,
    /// YouTube downloads through yt-dlp (off until turned on).
    pub youtube: Arc<sources::youtube::YouTube>,
    /// Listening history for the stats page.
    pub plays: Arc<library::stats::PlayTracker>,
    pub sleep: Arc<sleep::SleepTimer>,
}

fn serve_cover(app: &AppHandle, uri: &tauri::http::Uri) -> Option<Vec<u8>> {
    let id = uri.path().trim_start_matches('/');
    let full = uri.query().map(|q| q.contains("s=full")).unwrap_or(false);
    let state = app.try_state::<AppState>()?;
    let path = state.library.covers.file(id, full)?;
    std::fs::read(path).ok()
}

/// Custom GIFs, videos and background photos, with HTTP Range support (video needs it to loop and
/// seek reliably).
fn serve_media(app: &AppHandle, uri: &tauri::http::Uri, range: Option<&str>) -> Option<library::media::Served> {
    let id = uri.path().trim_start_matches('/');
    let state = app.try_state::<AppState>()?;
    let (path, mime) = state.library.media.file(id)?;
    library::media::serve(&path, mime, range).ok()
}

/// Shrinks the window to fit the screen's work area and centres it there. On small or scaled
/// screens the configured size can be taller than the space above the taskbar.
fn fit_to_work_area(w: &tauri::WebviewWindow) {
    let (Ok(Some(monitor)), Ok(size)) = (w.current_monitor(), w.inner_size()) else { return };
    let (Ok(outer), Ok(inner)) = (w.outer_position(), w.inner_position()) else { return };
    let area = monitor.work_area();
    let width = size.width.min(area.size.width);
    let height = size.height.min(area.size.height);
    if (width, height) != (size.width, size.height) {
        let _ = w.set_size(tauri::PhysicalSize::new(width, height));
    }
    // The outer frame includes invisible resize borders; place the visible part.
    let x = area.position.x + ((area.size.width - width) / 2) as i32 - (inner.x - outer.x);
    let y = area.position.y + ((area.size.height - height) / 2) as i32 - (inner.y - outer.y);
    let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    // WebView2 keeps reporting the page as visible while the window is hidden; tell the
    // frontend so GIF and video artwork stops and starts with the window.
    let _ = app.emit("window:hidden", false);
    island::main_changed(app);
}

/// Audio files/folders passed on the command line ("Open with", drag onto the .exe).
fn path_args(args: &[String]) -> Vec<String> {
    args.iter()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .filter(|a| {
            let p = std::path::Path::new(a);
            p.is_dir() || library::model::is_audio_file(p)
        })
        .cloned()
        .collect()
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let toggle = MenuItem::with_id(app, "toggle", "Play / Pause", true, None::<&str>)?;
    let prev = MenuItem::with_id(app, "prev", "Previous", true, None::<&str>)?;
    let next = MenuItem::with_id(app, "next", "Next", true, None::<&str>)?;
    let show = MenuItem::with_id(app, "show", "Show Undertone", true, None::<&str>)?;
    // Also the way back when the island lets clicks through and can't be right-clicked.
    let island = MenuItem::with_id(app, "island", "Dynamic Island settings", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&toggle, &prev, &next, &sep, &show, &island, &sep2, &quit])?;
    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Undertone")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let state = app.try_state::<AppState>();
            match event.id.as_ref() {
                "toggle" => state.map(|s| s.engine.send(Command::Toggle)).unwrap_or_default(),
                "prev" => state.map(|s| s.engine.send(Command::Previous)).unwrap_or_default(),
                "next" => state.map(|s| s.engine.send(Command::Next)).unwrap_or_default(),
                "show" => show_main(app),
                "island" => {
                    show_main(app);
                    let _ = app.emit_to("main", "ui:island-settings", ());
                }
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,symphonia=warn,lofty=error"))
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            show_main(app);
            let paths = path_args(&args);
            if !paths.is_empty() {
                commands::open_external(app, paths);
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        // Signed auto-updates from the GitHub Releases feed in tauri.conf.json.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(hotkeys::plugin())
        // Album art is served from the local cache through a custom protocol (no network).
        .register_asynchronous_uri_scheme_protocol("cover", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let uri = request.uri().clone();
            std::thread::spawn(move || {
                let resp = match serve_cover(&app, &uri) {
                    Some(bytes) => tauri::http::Response::builder()
                        .header("Content-Type", "image/jpeg")
                        .header("Cache-Control", "max-age=31536000, immutable")
                        .header("Access-Control-Allow-Origin", "*")
                        .body(bytes),
                    None => tauri::http::Response::builder().status(404).body(Vec::new()),
                };
                if let Ok(r) = resp {
                    responder.respond(r);
                }
            });
        })
        .register_asynchronous_uri_scheme_protocol("media", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let uri = request.uri().clone();
            let range = request.headers().get("range").and_then(|v| v.to_str().ok()).map(str::to_string);
            std::thread::spawn(move || {
                let resp = match serve_media(&app, &uri, range.as_deref()) {
                    Some(served) => {
                        let mut b = tauri::http::Response::builder().status(served.status);
                        for (k, v) in served.headers {
                            b = b.header(k, v);
                        }
                        b.body(served.body)
                    }
                    None => tauri::http::Response::builder()
                        .status(404)
                        .header("Access-Control-Allow-Origin", "*")
                        .body(Vec::new()),
                };
                if let Ok(r) = resp {
                    responder.respond(r);
                }
            });
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            // The island window alone mustn't keep the app running.
            if let WindowEvent::Destroyed = event {
                window.app_handle().exit(0);
            }
            // Minimising and restoring resize the window; restoring also focuses it.
            if let WindowEvent::Resized(_) | WindowEvent::Focused(_) = event {
                island::main_changed(window.app_handle());
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                let to_tray = window
                    .app_handle()
                    .try_state::<AppState>()
                    .map(|s| s.settings.get().close_to_tray)
                    .unwrap_or(false);
                if to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                    let _ = window.app_handle().emit("window:hidden", true);
                    island::main_changed(window.app_handle());
                }
            }
        })
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            let data_dir = app.path().app_data_dir()?;
            let cache_dir = app.path().app_cache_dir()?;
            for d in [&config_dir, &data_dir, &cache_dir] {
                std::fs::create_dir_all(d)?;
            }
            let settings = SettingsStore::load(config_dir.join("settings.json"));
            let s = settings.get();

            let lib_handle = app.handle().clone();
            let library = Library::open(
                &data_dir.join("library.db"),
                cache_dir.join("covers"),
                cache_dir.join("media"),
                Arc::new(move |ev| match ev {
                    LibraryEvent::Progress(p) => {
                        let _ = lib_handle.emit("library:progress", p);
                    }
                    LibraryEvent::ScanStarted => {
                        let _ = lib_handle.emit("library:scan-started", ());
                    }
                    LibraryEvent::Finished(summary) => {
                        let _ = lib_handle.emit("library:changed", summary);
                    }
                }),
            )
            .map_err(|e| format!("cannot open library database: {e}"))?;
            // Custom media nothing uses any more (removed backgrounds, refused imports).
            let skin: HashSet<String> = s.appearance.background.iter().map(|b| b.media_id.clone()).collect();
            match library.sweep_media(&skin) {
                Ok(0) => {}
                Ok(n) => log::info!("removed {n} unused media file(s)"),
                Err(e) => log::warn!("media clean-up failed: {e}"),
            }
            library.set_watching(s.watch_folders);
            // Quick incremental rescan on startup picks up changes made while the app was closed.
            library.request_scan(library::ScanRequest::All { force: false });

            // Restore last session's queue (playback resumes at the saved position on Play).
            let queue_file = data_dir.join("queue.json");
            let (queue, resume_pos) = persist::load_queue(&queue_file, &library);
            let resume_uid = queue.current_uid();

            // Engine events fan out to the UI, the Windows media overlay and the tray tooltip.
            let smtc_tx: Arc<Mutex<Option<Sender<SmtcUpdate>>>> = Arc::new(Mutex::new(None));
            let last_playback = Mutex::new((PlayState::Stopped, 0.0f64, Instant::now()));
            let handle = app.handle().clone();
            let smtc_slot = smtc_tx.clone();
            let plays = library::stats::PlayTracker::new(library.clone());
            let tracker = plays.clone();
            let engine = Engine::new(
                EngineConfig {
                    mode: s.output_mode,
                    device_id: s.device_id.clone(),
                    volume: s.volume,
                    eq: s.eq.clone(),
                    rg_mode: s.replay_gain,
                    rg_preamp: s.replay_gain_preamp,
                    crossfade: s.crossfade,
                },
                queue,
                move |ev| match ev {
                    EngineEvent::Status(st) => {
                        tracker.update(st.current_uid, st.track.as_ref(), st.state == PlayState::Playing);
                        if let Some(tx) = smtc_slot.lock().as_ref() {
                            let mut last = last_playback.lock();
                            let expected = last.1
                                + if last.0 == PlayState::Playing { last.2.elapsed().as_secs_f64() } else { 0.0 };
                            // Only state changes and seeks need to reach the overlay.
                            if st.state != last.0 || (st.position - expected).abs() > 1.5 {
                                let _ = tx.send(SmtcUpdate::Playback { state: st.state, position: st.position });
                            }
                            *last = (st.state, st.position, Instant::now());
                        }
                        let _ = handle.emit("player:status", st);
                    }
                    EngineEvent::TrackChanged(item) => {
                        if let Some(item) = &item {
                            let t = &item.track;
                            let artist = if t.artist.is_empty() { t.album_artist.clone() } else { t.artist.clone() };
                            if let Some(tray) = handle.tray_by_id("main") {
                                let _ = tray.set_tooltip(Some(format!("{} — {}", t.title, artist)));
                            }
                            if let Some(tx) = smtc_slot.lock().as_ref() {
                                let cover = t.cover_id.as_deref().and_then(|id| {
                                    handle.try_state::<AppState>().and_then(|s| s.library.covers.file(id, true))
                                });
                                let _ = tx.send(SmtcUpdate::Track {
                                    title: t.title.clone(),
                                    artist,
                                    album: t.album.clone(),
                                    cover,
                                    duration: t.duration,
                                });
                            }
                        }
                        let _ = handle.emit("player:track", item);
                    }
                    EngineEvent::Error(msg) => {
                        let _ = handle.emit("player:error", msg);
                    }
                },
            );
            engine.set_effects(s.effects.speed, s.effects.reverb);
            if let (Some(uid), Some(position)) = (resume_uid, resume_pos) {
                engine.send(Command::SetResume { uid, position });
            }

            // Media overlay + hardware media keys (System Media Transport Controls).
            if let Some(window) = app.get_webview_window("main") {
                fit_to_work_area(&window);
                if let Ok(hwnd) = window.hwnd() {
                    let raise_handle = app.handle().clone();
                    let tx = smtc::start(hwnd.0 as isize, engine.clone(), move || show_main(&raise_handle));
                    *smtc_tx.lock() = tx;
                }
            }

            if let Err(e) = build_tray(app.handle()) {
                log::warn!("system tray unavailable: {e}");
            }

            // Persist the queue periodically when it changed (and on exit).
            let queue_dirty = Arc::new(AtomicBool::new(false));
            {
                let engine = engine.clone();
                let dirty = queue_dirty.clone();
                let file = queue_file.clone();
                std::thread::Builder::new().name("queue-saver".into()).spawn(move || loop {
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    let playing = engine.status().state == PlayState::Playing;
                    if dirty.swap(false, Ordering::Relaxed) || playing {
                        persist::save_queue(&file, &engine);
                    }
                })?;
            }

            // Soulseek stays offline until the user signs in (or saved a sign-in earlier).
            let soulseek = sources::soulseek::Soulseek::new(app.handle().clone(), &app.config().identifier);
            soulseek.sign_in_saved(s.soulseek.clone());
            // A finished download that was found for an imported playlist entry replaces that
            // entry in every playlist that has it (recorded as the user's choice).
            {
                let (lib, handle) = (library.clone(), app.handle().clone());
                soulseek.set_on_linked_done(Arc::new(move |key, path| {
                    let path = path.to_string_lossy().to_string();
                    match library::interchange::decide(&mut lib.db.lock(), &key, "accept", Some(&path)) {
                        Ok(_) => {
                            let _ = handle.emit("playlists:changed", ());
                        }
                        Err(e) => log::warn!("could not link {path} to its playlist entry: {e:#}"),
                    }
                }));
            }
            let finder = sources::missing::Finder::new(app.handle().clone());
            let youtube = sources::youtube::YouTube::new(app.handle().clone(), cache_dir.join("tools"), s.youtube.enabled);
            soulseek.set_youtube(youtube.clone());
            if s.youtube.enabled {
                youtube.set_enabled(true);
            }

            let sleep = sleep::SleepTimer::new(engine.clone(), app.handle().clone());
            island::set_click_through(s.island_options.click_through);
            if s.island {
                island::open(app.handle());
            }
            app.manage(AppState { engine, settings, library, queue_dirty, queue_file, soulseek, finder, versions: Default::default(), youtube, plays, sleep });

            // Registering waits on the main thread, which is busy here.
            let handle = app.handle().clone();
            let keys = s.hotkeys.clone();
            tauri::async_runtime::spawn_blocking(move || hotkeys::apply(&handle, &keys));
            if s.phone_sync.enabled {
                let handle = app.handle().clone();
                let sync = s.phone_sync.clone();
                tauri::async_runtime::spawn_blocking(move || phone::apply(&handle, &sync));
            }

            // Files passed on the command line start playing right away.
            let args: Vec<String> = std::env::args().collect();
            let paths = path_args(&args);
            if !paths.is_empty() {
                commands::open_external(app.handle(), paths);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::play_files,
            commands::toggle_play,
            commands::play,
            commands::pause,
            commands::stop,
            commands::next_track,
            commands::previous_track,
            commands::seek,
            commands::set_volume,
            commands::set_muted,
            commands::list_devices,
            commands::set_output,
            commands::get_queue,
            commands::set_shuffle,
            commands::set_repeat,
            commands::get_settings,
            commands::update_settings,
            commands::get_library_folders,
            commands::add_library_folder,
            commands::remove_library_folder,
            commands::rescan_library,
            commands::is_scanning,
            commands::get_tracks,
            commands::play_tracks,
            commands::enqueue_tracks,
            commands::play_paths,
            commands::play_queue_item,
            commands::remove_from_queue,
            commands::move_queue_item,
            commands::clear_queue,
            commands::get_playlists,
            commands::create_playlist,
            commands::rename_playlist,
            commands::delete_playlist,
            commands::get_playlist,
            commands::add_to_playlist,
            commands::set_playlist_paths,
            commands::import_playlist,
            commands::export_playlist,
            commands::export_unmatched_report,
            commands::get_playlist_review,
            commands::get_recommendations,
            commands::find_similar_online,
            commands::add_reference_to_playlist,
            commands::decide_match,
            commands::save_queue_as_playlist,
            commands::set_eq,
            commands::get_eq_presets,
            commands::save_eq_preset,
            commands::delete_eq_preset,
            commands::import_autoeq,
            commands::open_url,
            commands::show_main_window,
            commands::island_main_away,
            commands::island_set_bounds,
            commands::island_set_pill,
            commands::get_hotkey_problems,
            commands::pause_hotkeys,
            commands::get_phone_sync,
            commands::new_phone_link,
            commands::set_effects,
            commands::island_set_panel,
            commands::app_ready,
            commands::quit_app,
            commands::update_playlist,
            commands::set_playlist_pinned,
            commands::reorder_playlists,
            commands::set_playlist_cover,
            commands::stage_media,
            commands::commit_media,
            commands::discard_media,
            commands::get_media,
            commands::get_lyrics,
            commands::search_lyrics,
            commands::remove_found_lyrics,
            commands::get_stats,
            commands::clear_play_history,
            commands::get_sleep_timer,
            commands::set_sleep_timer,
            commands::set_playlist_background,
            commands::set_tracks_cover_hidden,
            commands::set_tracks_cover,
            commands::delete_tracks_from_disk,
            commands::get_deleted_tracks,
            commands::restore_deleted_tracks,
            commands::forget_deleted_tracks,
            commands::export_backup,
            commands::import_backup,
            sources::commands::slsk_status,
            sources::commands::slsk_sign_in,
            sources::commands::slsk_sign_out,
            sources::commands::slsk_reconnect,
            sources::commands::yt_status,
            sources::commands::yt_set_enabled,
            sources::commands::yt_install,
            sources::commands::slsk_search,
            sources::commands::slsk_stop_search,
            sources::commands::slsk_downloads,
            sources::commands::slsk_download,
            sources::commands::slsk_cancel,
            sources::commands::slsk_retry,
            sources::commands::slsk_clear_finished,
            sources::commands::slsk_settings,
            sources::commands::slsk_update_settings,
            sources::commands::check_link,
            sources::commands::import_link,
            sources::commands::find_missing,
            sources::commands::stop_find,
            sources::commands::skip_find,
            sources::commands::find_results,
            sources::commands::download_found,
            sources::commands::find_versions,
            sources::commands::cancel_versions,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Undertone")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app.try_state::<AppState>() {
                    persist::save_queue(&state.queue_file, &state.engine);
                    state.plays.finish(&state.library);
                    state.engine.shutdown();
                }
            }
        });
}
