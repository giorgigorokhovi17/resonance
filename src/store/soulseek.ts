import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { api } from "../lib/api";
import type { SlskJob, SlskSearchUpdate, SlskStatus } from "../lib/types";
import { usePlayer } from "./player";

interface SoulseekStore {
  status: SlskStatus;
  jobs: SlskJob[];
  /** Latest results of the current search (kept while you browse other pages). */
  search: SlskSearchUpdate | null;
  searchId: number | null;
  startSearch: (query: string) => Promise<void>;
}

export const useSoulseek = create<SoulseekStore>((set) => ({
  status: { state: "signedOut", username: null, remembered: false, error: null },
  jobs: [],
  search: null,
  searchId: null,
  startSearch: async (query) => {
    try {
      const id = await api.slskSearch(query);
      set({ searchId: id, search: { id, query, users: [], fileCount: 0, done: false } });
    } catch (e) {
      usePlayer.getState().pushToast("error", String(e));
    }
  },
}));

let initialized = false;
export async function initSoulseek() {
  if (initialized) return;
  initialized = true;
  await listen<SlskStatus>("slsk:status", (e) => useSoulseek.setState({ status: e.payload }));
  await listen<SlskJob[]>("slsk:downloads", (e) => {
    const before = new Map(useSoulseek.getState().jobs.map((j) => [j.id, j.state]));
    useSoulseek.setState({ jobs: e.payload });
    const finished = e.payload.filter((j) => j.state === "done" && before.get(j.id) !== "done" && before.has(j.id));
    if (finished.length === 1) usePlayer.getState().pushToast("info", `Downloaded ${finished[0].name}.`);
    else if (finished.length > 1) usePlayer.getState().pushToast("info", `Downloaded ${finished.length} files.`);
  });
  await listen<SlskSearchUpdate>("slsk:search", (e) => {
    if (e.payload.id === useSoulseek.getState().searchId) useSoulseek.setState({ search: e.payload });
  });
  const [status, jobs] = await Promise.all([api.slskStatus(), api.slskDownloads()]);
  useSoulseek.setState({ status, jobs });
}
