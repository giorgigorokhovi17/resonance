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
    let window = match built {
        Ok(w) => w,
        Err(e) => {
            log::warn!("could not open the island: {e}");
            return;
        }
    };
    #[cfg(windows)]
    if let Ok(hwnd) = window.hwnd() {
        on_top::set(hwnd.0 as isize);
        let _ = app.run_on_main_thread(on_top::hook);
    }
    #[cfg(not(windows))]
    let _ = window;
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
            #[cfg(windows)]
            on_top::raise();
            std::thread::sleep(Duration::from_millis(1500));
        }
    });
}

pub fn close(app: &AppHandle) {
    #[cfg(windows)]
    on_top::set(0);
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.destroy();
    }
}

/// Keeps the island above other always-on-top windows. Games in borderless or windowed mode make
/// themselves always-on-top when clicked, and the last one raised wins; so whenever another
/// window comes to the front, the island goes back on top (without taking the focus).
#[cfg(windows)]
mod on_top {
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, EVENT_SYSTEM_FOREGROUND, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
        WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
    };

    /// The island's window handle (0 while it's closed).
    static ISLAND: AtomicIsize = AtomicIsize::new(0);
    static HOOKED: AtomicBool = AtomicBool::new(false);

    pub fn set(hwnd: isize) {
        ISLAND.store(hwnd, Ordering::Relaxed);
    }

    pub fn raise() {
        let h = ISLAND.load(Ordering::Relaxed);
        if h == 0 {
            return;
        }
        // SAFETY: a window handle of ours; Windows ignores it if the window is gone.
        unsafe {
            let _ = SetWindowPos(
                HWND(h as *mut _),
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            );
        }
    }

    unsafe extern "system" fn on_foreground(_: HWINEVENTHOOK, _: u32, _: HWND, _: i32, _: i32, _: u32, _: u32) {
        raise();
    }

    /// Watches for other apps coming to the front. Runs on the main thread, whose message loop
    /// delivers the events; set up once and kept for the app's lifetime.
    pub fn hook() {
        if HOOKED.swap(true, Ordering::Relaxed) {
            return;
        }
        // SAFETY: a plain callback that only calls SetWindowPos; out of context, so it runs here.
        let h = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(on_foreground),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            )
        };
        if h.is_invalid() {
            log::warn!("could not watch for other apps coming to the front");
            HOOKED.store(false, Ordering::Relaxed);
        }
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
