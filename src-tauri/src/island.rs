//! The Dynamic Island: a small always-on-top window at the top of the screen with the playing
//! song (src/views/Island.tsx). Off until turned on in Settings. It hides itself while another
//! app is full screen (games, videos, presentations).

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "island";

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
            std::thread::sleep(Duration::from_millis(1500));
        }
    });
}

pub fn close(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.destroy();
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
