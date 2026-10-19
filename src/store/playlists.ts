import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { api } from "../lib/api";
import type { Playlist } from "../lib/types";

interface PlaylistStore {
  playlists: Playlist[];
  /** Bumped on every change so open playlist views refetch. */
  version: number;
  load: () => Promise<void>;
}

export const usePlaylists = create<PlaylistStore>((set, get) => ({
  playlists: [],
  version: 0,
  load: async () => set({ playlists: await api.getPlaylists(), version: get().version + 1 }),
}));

let initialized = false;
export async function initPlaylists() {
  if (initialized) return;
  initialized = true;
  await listen("playlists:changed", () => usePlaylists.getState().load());
  await usePlaylists.getState().load();
}
