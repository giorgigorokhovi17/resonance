import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { useUi } from "../store/ui";

let restore: { w: number; h: number; maximized: boolean } | null = null;

/** Switch between the full window and a compact always-on-top mini player. */
export async function toggleMiniPlayer() {
  const win = getCurrentWindow();
  const ui = useUi.getState();
  if (!ui.mini) {
    const maximized = await win.isMaximized();
    if (maximized) await win.unmaximize();
    const size = (await win.innerSize()).toLogical(await win.scaleFactor());
    restore = { w: size.width, h: size.height, maximized };
    ui.setNowPlaying(false);
    ui.setMini(true);
    await win.setResizable(false);
    await win.setSize(new LogicalSize(440, 120));
    await win.setAlwaysOnTop(true);
  } else {
    ui.setMini(false);
    await win.setAlwaysOnTop(false);
    await win.setResizable(true);
    const r = restore ?? { w: 1280, h: 820, maximized: false };
    await win.setSize(new LogicalSize(Math.max(r.w, 900), Math.max(r.h, 600)));
    if (r.maximized) await win.maximize();
  }
}
