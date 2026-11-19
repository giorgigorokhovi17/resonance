import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { api } from "../lib/api";
import type { LovedEntry } from "../lib/types";
import { usePlayer } from "./player";

const key = (path: string) => path.toLowerCase();

interface LovedStore {
  /** Lower-cased file paths of loved songs. */
  set: Set<string>;
  /** Loved songs with timestamps, newest first. */
  entries: LovedEntry[];
  load: () => Promise<void>;
  setLoved: (path: string, loved: boolean) => Promise<void>;
  toggle: (path: string) => Promise<boolean>;
}

export const useLoved = create<LovedStore>((set, get) => ({
  set: new Set(),
  entries: [],
  load: async () => {
    const entries = await api.lovedEntries();
    set({ entries, set: new Set(entries.map((e) => key(e.path))) });
  },
  setLoved: async (path, loved) => {
    const k = key(path);
    const prev = get();
    const nextSet = new Set(prev.set);
    let nextEntries: LovedEntry[];
    if (loved) {
      nextSet.add(k);
      const now = Math.floor(Date.now() / 1000);
      nextEntries = [{ path, lovedAt: now }, ...prev.entries.filter((e) => key(e.path) !== k)];
    } else {
      nextSet.delete(k);
      nextEntries = prev.entries.filter((e) => key(e.path) !== k);
    }
    set({ set: nextSet, entries: nextEntries });
    try {
      await api.setLoved(path, loved);
    } catch (e) {
      usePlayer.getState().pushToast("error", String(e));
      await get().load();
    }
  },
  toggle: async (path) => {
    const next = !get().set.has(key(path));
    await get().setLoved(path, next);
    return next;
  },
}));

export function useIsLoved(path: string | null | undefined): boolean {
  return useLoved((s) => (path ? s.set.has(key(path)) : false));
}

let initialized = false;
export async function initLoved() {
  if (initialized) return;
  initialized = true;
  await listen("loved:changed", () => useLoved.getState().load());
  await useLoved.getState().load();
}
