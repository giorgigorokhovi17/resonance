import { getVersion } from "@tauri-apps/api/app";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { create } from "zustand";

/**
 * Auto-update state. Updates come from the signed GitHub Releases feed configured in
 * src-tauri/tauri.conf.json (plugins.updater).
 */
interface UpdaterState {
  phase: "idle" | "checking" | "none" | "available" | "downloading" | "installing" | "error";
  current: string;
  update: Update | null;
  /** 0–1 while downloading (null if the size is unknown). */
  progress: number | null;
  error: string | null;
  dismissed: boolean;
  check: (silent?: boolean) => Promise<void>;
  install: () => Promise<void>;
  dismiss: () => void;
}

export const useUpdater = create<UpdaterState>((set, get) => ({
  phase: "idle",
  current: "",
  update: null,
  progress: null,
  error: null,
  dismissed: false,
  check: async (silent = false) => {
    if (get().phase === "checking" || get().phase === "downloading" || get().phase === "installing") return;
    set({ phase: "checking", error: null, current: get().current || (await getVersion().catch(() => "")) });
    try {
      const update = await check();
      set({ update, phase: update ? "available" : "none", dismissed: false });
    } catch (e) {
      // A silent startup check stays quiet when offline or before the first release exists.
      set({ phase: silent ? "idle" : "error", error: String(e) });
    }
  },
  install: async () => {
    const update = get().update;
    if (!update) return;
    set({ phase: "downloading", progress: 0, error: null });
    try {
      let total = 0;
      let got = 0;
      await update.downloadAndInstall((e) => {
        if (e.event === "Started") total = e.data.contentLength ?? 0;
        else if (e.event === "Progress") {
          got += e.data.chunkLength;
          set({ progress: total ? got / total : null });
        } else if (e.event === "Finished") set({ phase: "installing" });
      });
      // On Windows the installer usually closes the app itself; relaunch covers the rest.
      await relaunch();
    } catch (e) {
      set({ phase: "error", error: String(e) });
    }
  },
  dismiss: () => set({ dismissed: true }),
}));

/** Quiet check a few seconds after startup, then every 6 hours while the app stays open. */
export function initUpdater() {
  getVersion()
    .then((current) => useUpdater.setState({ current }))
    .catch(() => {});
  if (import.meta.env.DEV) return;
  window.setTimeout(() => useUpdater.getState().check(true), 5000);
  window.setInterval(() => useUpdater.getState().check(true), 6 * 60 * 60 * 1000);
}
