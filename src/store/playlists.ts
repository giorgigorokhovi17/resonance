import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { api } from "../lib/api";
import type { Playlist } from "../lib/types";
import { usePlayer } from "./player";

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

const toastErr = (e: unknown) => usePlayer.getState().pushToast("error", String(e));

/**
 * Move the playlist at `from` to `to` in the sidebar order. Pinned playlists stay above the
 * rest, so dropping one among the pinned ones pins it, and dragging a pinned one below them
 * unpins it.
 */
export async function movePlaylist(from: number, to: number) {
  const list = [...usePlaylists.getState().playlists];
  if (from === to || from < 0 || from >= list.length || to < 0 || to >= list.length) return;
  const [p] = list.splice(from, 1);
  const pinnedOthers = list.filter((x) => x.pinned).length;
  const pinned = to < pinnedOthers || (to === pinnedOthers && p.pinned);
  list.splice(to, 0, { ...p, pinned });
  // Show the new order straight away; the backend's change event reloads it afterwards.
  usePlaylists.setState({ playlists: list });
  try {
    if (pinned !== p.pinned) await api.setPlaylistPinned(p.id, pinned);
    await api.reorderPlaylists(list.map((x) => x.id));
  } catch (e) {
    toastErr(e);
    usePlaylists.getState().load();
  }
}

/** Pinning puts the playlist at the bottom of the pinned ones; unpinning, at the top of the rest. */
export async function setPinned(p: Playlist, pinned: boolean) {
  const list = usePlaylists.getState().playlists.filter((x) => x.id !== p.id);
  const at = list.filter((x) => x.pinned).length;
  list.splice(at, 0, { ...p, pinned });
  usePlaylists.setState({ playlists: list });
  try {
    await api.setPlaylistPinned(p.id, pinned);
    await api.reorderPlaylists(list.map((x) => x.id));
  } catch (e) {
    toastErr(e);
    usePlaylists.getState().load();
  }
}
