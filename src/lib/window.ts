import { currentMonitor, getCurrentWindow, LogicalPosition, LogicalSize } from "@tauri-apps/api/window";
import { useUi } from "../store/ui";

const MINI = { w: 440, h: 120 };
const MINI_POS_KEY = "resonance.miniPos";
const MARGIN = 16;
// Mini mode lives in page state, but the window keeps its mini size if the page reloads
// (WebView2 can do that on its own). Remember it for the session so the UI matches.
const SESSION_KEY = "resonance.miniSession";

type Rect = { x: number; y: number; w: number; h: number };
let restore: (Rect & { maximized: boolean }) | null = null;

/** The screen area outside the taskbar, in logical pixels. */
async function workArea(): Promise<Rect | null> {
  const m = await currentMonitor();
  if (!m) return null;
  const sf = m.scaleFactor;
  return {
    x: m.workArea.position.x / sf,
    y: m.workArea.position.y / sf,
    w: m.workArea.size.width / sf,
    h: m.workArea.size.height / sf,
  };
}

/** Width of the invisible resize border: setPosition places the outer frame, not the content. */
async function frameOffset(sf: number): Promise<{ x: number; y: number }> {
  const win = getCurrentWindow();
  const [outer, inner] = await Promise.all([win.outerPosition(), win.innerPosition()]);
  return { x: (inner.x - outer.x) / sf, y: (inner.y - outer.y) / sf };
}

/** Moves the window so its visible content starts at (x, y), in logical pixels. */
async function placeContent(x: number, y: number) {
  const win = getCurrentWindow();
  const off = await frameOffset(await win.scaleFactor());
  await win.setPosition(new LogicalPosition(x - off.x, y - off.y));
}

const clamp = (v: number, lo: number, hi: number) => Math.min(Math.max(v, lo), Math.max(lo, hi));

function loadMiniPos(): { x: number; y: number } | null {
  try {
    const p = JSON.parse(localStorage.getItem(MINI_POS_KEY) ?? "null");
    return typeof p?.x === "number" && typeof p?.y === "number" ? p : null;
  } catch {
    return null;
  }
}

function saveMiniPos(p: { x: number; y: number }) {
  try {
    localStorage.setItem(MINI_POS_KEY, JSON.stringify(p));
  } catch {
    // Not critical: the mini player then opens in the corner next time.
  }
}

/** After a page reload, goes back to the mini layout if the window is still the mini player. */
export function resumeMiniAfterReload() {
  try {
    const r = JSON.parse(sessionStorage.getItem(SESSION_KEY) ?? "null");
    if (r && typeof r.w === "number") {
      restore = r;
      useUi.getState().setMini(true);
    }
  } catch {
    // ignore
  }
}

/**
 * Switch between the full window and a compact always-on-top mini player. The mini player
 * opens where it was last left (bottom-right corner the first time); the full window comes
 * back at its old size and place, kept above the taskbar.
 */
export async function toggleMiniPlayer() {
  const win = getCurrentWindow();
  const ui = useUi.getState();
  if (!ui.mini) {
    const maximized = await win.isMaximized();
    if (maximized) await win.unmaximize();
    const sf = await win.scaleFactor();
    const size = (await win.innerSize()).toLogical(sf);
    const pos = (await win.innerPosition()).toLogical(sf);
    restore = { x: pos.x, y: pos.y, w: size.width, h: size.height, maximized };
    try {
      sessionStorage.setItem(SESSION_KEY, JSON.stringify(restore));
    } catch {
      // Without it, a reload while mini shows the full layout until Ctrl+M.
    }
    ui.setNowPlaying(false);
    ui.setMini(true);
    await win.setResizable(false);
    await win.setSize(new LogicalSize(MINI.w, MINI.h));
    const area = await workArea();
    if (area) {
      const saved = loadMiniPos();
      const x = saved?.x ?? area.x + area.w - MINI.w - MARGIN;
      const y = saved?.y ?? area.y + area.h - MINI.h - MARGIN;
      await placeContent(clamp(x, area.x, area.x + area.w - MINI.w), clamp(y, area.y, area.y + area.h - MINI.h));
    }
    await win.setAlwaysOnTop(true);
  } else {
    const sf = await win.scaleFactor();
    const pos = (await win.innerPosition()).toLogical(sf);
    saveMiniPos({ x: pos.x, y: pos.y });
    try {
      sessionStorage.removeItem(SESSION_KEY);
    } catch {
      // ignore
    }
    const area = await workArea();
    ui.setMini(false);
    await win.setAlwaysOnTop(false);
    await win.setResizable(true);
    const r = restore ?? { x: NaN, y: NaN, w: 1280, h: 820, maximized: false };
    let w = Math.max(r.w, 900);
    let h = Math.max(r.h, 600);
    if (area) {
      w = Math.min(w, area.w);
      h = Math.min(h, area.h);
    }
    await win.setSize(new LogicalSize(w, h));
    if (area) {
      const x = Number.isFinite(r.x) ? r.x : area.x + (area.w - w) / 2;
      const y = Number.isFinite(r.y) ? r.y : area.y + (area.h - h) / 2;
      await placeContent(clamp(x, area.x, area.x + area.w - w), clamp(y, area.y, area.y + area.h - h));
    }
    if (r.maximized) await win.maximize();
  }
}
