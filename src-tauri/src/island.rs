//! The Dynamic Island: a small always-on-top window with the playing song (src/views/Island.tsx).
//! Off until turned on in Settings. By default it shows only while Undertone is minimised or in
//! the tray, and it hides itself while another app is full screen (games, videos, presentations).

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use parking_lot::Mutex;
use crate::audio::viz::Analyzer;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "island";

/// The last "main window away" state sent to the island.
static AWAY: AtomicBool = AtomicBool::new(false);
/// The island lets clicks through (Settings → Dynamic Island), except while Alt is held.
static CLICK_THROUGH: AtomicBool = AtomicBool::new(false);
/// Where the pill is inside the window (x, y, width, height in physical pixels). The window is
/// as big as the opened island so it never resizes while the pill grows and shrinks; clicks
/// anywhere else in it go to the app underneath.
static PILL: Mutex<Option<[i32; 4]>> = Mutex::new(None);

pub fn set_click_through(on: bool) {
    CLICK_THROUGH.store(on, Ordering::Relaxed);
}

pub fn set_pill(rect: [i32; 4]) {
    *PILL.lock() = Some(rect);
}

/// Opens the island window (hidden; the page shows it once it has a song) and starts watching
/// for full-screen apps and the Alt key.
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
        // Layered once, here, so letting clicks through later is a single style bit. Having tao
        // switch click-through rewrites every style and refreshes the frame, which waits on the
        // webview's window in its own process; done each time the pointer crossed the pill's
        // edge, that could lock up the app.
        let _ = window.set_ignore_cursor_events(true);
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
    // The visualizer: band levels of what's playing, ~30 times a second while the island shows.
    let handle = app.clone();
    let _ = std::thread::Builder::new().name("island-viz".into()).spawn(move || {
        let mut analyzer = Analyzer::default();
        let mut still = false;
        while let Some(w) = handle.get_webview_window(LABEL) {
            let shared = handle.try_state::<crate::AppState>().map(|s| s.engine.shared.clone());
            let Some(shared) = shared.filter(|_| w.is_visible().unwrap_or(false)) else {
                if let Some(s) = handle.try_state::<crate::AppState>() {
                    s.engine.shared.viz.enabled.store(false, Ordering::Relaxed);
                }
                std::thread::sleep(Duration::from_millis(250));
                continue;
            };
            shared.viz.enabled.store(true, Ordering::Relaxed);
            let playing = !shared.paused.load(Ordering::Relaxed);
            let levels = if playing { analyzer.levels(&shared.viz) } else { analyzer.quiet() };
            // Once the bars have settled while paused, stop sending.
            let settled = !playing && levels.iter().all(|l| *l == 0.0);
            if !(settled && still) {
                let _ = handle.emit_to(LABEL, "island:levels", levels);
            }
            still = settled;
            std::thread::sleep(Duration::from_millis(33));
        }
        if let Some(s) = handle.try_state::<crate::AppState>() {
            s.engine.shared.viz.enabled.store(false, Ordering::Relaxed);
        }
    });
    // Clicks reach the island only over the pill, and with click-through on only while Alt is
    // held (say, next song in a game).
    let handle = app.clone();
    let _ = std::thread::Builder::new().name("island-clicks".into()).spawn(move || {
        let mut last = None;
        while let Some(_w) = handle.get_webview_window(LABEL) {
            let through = CLICK_THROUGH.load(Ordering::Relaxed);
            let ignore = !pointer_on_pill() || (through && !alt_down());
            // Set every time: showing or hiding the window has tao put its own styles back.
            #[cfg(windows)]
            on_top::let_clicks_through(ignore);
            if last != Some(ignore) {
                last = Some(ignore);
                #[cfg(not(windows))]
                let _ = _w.set_ignore_cursor_events(ignore);
                // The page can't see the pointer leave once clicks go through.
                let _ = handle.emit_to(LABEL, "island:clickable", !ignore);
            }
            std::thread::sleep(Duration::from_millis(30));
        }
    });
}

/// The pointer is over the pill (or the page hasn't said where the pill is yet).
fn pointer_on_pill() -> bool {
    let Some(rect) = *PILL.lock() else { return true };
    #[cfg(windows)]
    return on_top::pointer_in(rect);
    #[cfg(not(windows))]
    {
        let _ = rect;
        true
    }
}

/// Moves and resizes the island in one step (physical pixels), so it never shows at the new size
/// in the old place.
pub fn set_bounds(app: &AppHandle, x: i32, y: i32, w: u32, h: u32) {
    #[cfg(windows)]
    if on_top::set_bounds(x, y, w, h) {
        return;
    }
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.set_size(tauri::PhysicalSize::new(w, h));
        let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
    }
}

/// Alt is held down, in any app.
#[cfg(windows)]
pub fn alt_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_MENU};
    // SAFETY: reads the key state; no pointers involved.
    unsafe { GetAsyncKeyState(i32::from(VK_MENU.0)) as u16 & 0x8000 != 0 }
}

#[cfg(not(windows))]
pub fn alt_down() -> bool {
    false
}

pub fn close(app: &AppHandle) {
    #[cfg(windows)]
    on_top::set(0);
    *PILL.lock() = None;
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

    use windows::Win32::Foundation::{HWND, POINT, RECT};
    use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetWindowLongPtrW, GetWindowRect, SetWindowLongPtrW, SetWindowPos, EVENT_SYSTEM_FOREGROUND,
        GWL_EXSTYLE, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER,
        WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WS_EX_LAYERED, WS_EX_TRANSPARENT,
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

    /// The pointer is inside `rect` (x, y, width, height), relative to the island window.
    /// Lets clicks through the window (or not) by switching only WS_EX_TRANSPARENT, keeping it
    /// layered (it needs both to let clicks through). Only the window's own style changes: no
    /// frame refresh, nothing sent to the webview's window. Does nothing when already so.
    pub fn let_clicks_through(through: bool) {
        let hwnd = ISLAND.load(Ordering::Relaxed);
        if hwnd == 0 {
            return;
        }
        let hwnd = HWND(hwnd as *mut _);
        // SAFETY: a window handle of ours; reading and setting its extended style.
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let (layered, transparent) = (WS_EX_LAYERED.0 as isize, WS_EX_TRANSPARENT.0 as isize);
            let want = if through { style | layered | transparent } else { (style | layered) & !transparent };
            if want != style {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, want);
            }
        }
    }

    pub fn pointer_in(rect: [i32; 4]) -> bool {
        let hwnd = ISLAND.load(Ordering::Relaxed);
        if hwnd == 0 {
            return false;
        }
        let mut win = RECT::default();
        let mut p = POINT::default();
        // SAFETY: a window handle of ours and two plain out-structs.
        let ok = unsafe { GetWindowRect(HWND(hwnd as *mut _), &mut win).is_ok() && GetCursorPos(&mut p).is_ok() };
        let [x, y, w, h] = rect;
        let (left, top) = (win.left + x, win.top + y);
        ok && p.x >= left && p.x < left + w && p.y >= top && p.y < top + h
    }

    /// Position and size together; false when the island isn't open.
    pub fn set_bounds(x: i32, y: i32, w: u32, h: u32) -> bool {
        let hwnd = ISLAND.load(Ordering::Relaxed);
        if hwnd == 0 {
            return false;
        }
        let (Ok(w), Ok(h)) = (i32::try_from(w), i32::try_from(h)) else { return false };
        // SAFETY: a window handle of ours; Windows ignores it if the window is gone.
        unsafe { SetWindowPos(HWND(hwnd as *mut _), None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE).is_ok() }
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
