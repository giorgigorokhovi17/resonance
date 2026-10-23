//! The Dynamic Island: a small always-on-top window with the playing song (src/views/Island.tsx).
//! Off until turned on in Settings. By default it shows only while Undertone is minimised or in
//! the tray, and it hides itself while another app is full screen (games, videos, presentations).

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "island";

/// The last "main window away" state sent to the island.
static AWAY: AtomicBool = AtomicBool::new(false);

/// Opens the island window (hidden; the page shows it once it has a song) and starts watching
/// for full-screen apps.
pub fn open(app: &AppHandle) {
    if app.get_webview_window(LABEL).is_some() {
        return;
    }
    let built = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::default())
        .title("Undertone island")
        .inner_size(320.0, 56.0)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .focused(false)
        .visible(false)
        .build();
    if let Err(e) = built {
        log::warn!("could not open the island: {e}");
        return;
    }
    let handle = app.clone();
    let _ = std::thread::Builder::new().name("island-fullscreen".into()).spawn(move || {
        let mut last = None;
        while handle.get_webview_window(LABEL).is_some() {
            let busy = fullscreen_app();
            if last != Some(busy) {
                last = Some(busy);
                let _ = handle.emit_to(LABEL, "island:fullscreen", busy);
            }
            // Window events cover minimising and the tray; this catches anything they miss.
            main_changed(&handle);
            std::thread::sleep(Duration::from_millis(1500));
        }
    });
}

pub fn close(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.destroy();
    }
}

/// The main window is minimised or hidden in the tray.
pub fn main_away(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .map(|w| w.is_minimized().unwrap_or(false) || !w.is_visible().unwrap_or(true))
        .unwrap_or(true)
}

/// Tells the island when the main window is minimised, hidden or brought back.
pub fn main_changed(app: &AppHandle) {
    let away = main_away(app);
    if AWAY.swap(away, Ordering::Relaxed) != away {
        let _ = app.emit_to(LABEL, "island:main-away", away);
    }
}

/// Another app is full screen, or Windows is in presentation / do-not-disturb mode.
#[cfg(windows)]
fn fullscreen_app() -> bool {
    use windows::Win32::UI::Shell::{
        SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
    };
    // SAFETY: no arguments; returns a plain enum value.
    match unsafe { SHQueryUserNotificationState() } {
        Ok(s) => s == QUNS_BUSY || s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE,
        Err(_) => false,
    }
}

#[cfg(not(windows))]
fn fullscreen_app() -> bool {
    false
}
