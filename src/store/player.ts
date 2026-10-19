import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { api } from "../lib/api";
import type { PlayerStatus, QueueItem, QueueSnapshot, Settings } from "../lib/types";

export interface Toast {
  id: number;
  kind: "error" | "info";
  message: string;
}

interface PlayerStore {
  status: PlayerStatus | null;
  /** performance.now() when `status.position` was received, for smooth interpolation. */
  statusAt: number;
  queue: QueueSnapshot;
  settings: Settings | null;
  toasts: Toast[];
  setStatus: (s: PlayerStatus) => void;
  setQueue: (q: QueueSnapshot) => void;
  setSettings: (s: Settings) => void;
  pushToast: (kind: Toast["kind"], message: string) => void;
  dismissToast: (id: number) => void;
  /** Optimistic local volume so the slider never lags. */
  setLocalVolume: (v: number) => void;
}

let toastId = 0;

export const usePlayer = create<PlayerStore>((set) => ({
  status: null,
  statusAt: 0,
  queue: { items: [], currentUid: null, shuffle: false, repeat: "off" },
  settings: null,
  toasts: [],
  setStatus: (s) => set({ status: s, statusAt: performance.now() }),
  setQueue: (q) => set({ queue: q }),
  setSettings: (s) => set({ settings: s }),
  pushToast: (kind, message) =>
    set((st) => {
      const id = ++toastId;
      setTimeout(() => usePlayer.getState().dismissToast(id), kind === "error" ? 7000 : 3500);
      // Collapse duplicates so a failing device doesn't spam.
      const toasts = st.toasts.filter((t) => t.message !== message).slice(-3);
      return { toasts: [...toasts, { id, kind, message }] };
    }),
  dismissToast: (id) => set((st) => ({ toasts: st.toasts.filter((t) => t.id !== id) })),
  setLocalVolume: (v) =>
    set((st) => (st.status ? { status: { ...st.status, volume: v } } : {})),
}));

/** Current playback position, extrapolated between status events. */
export function livePosition(): number {
  const { status, statusAt } = usePlayer.getState();
  if (!status) return 0;
  if (status.state !== "playing") return status.position;
  const p = status.position + (performance.now() - statusAt) / 1000;
  return Math.min(p, status.duration || p);
}

export function currentItem(): QueueItem | undefined {
  const { queue } = usePlayer.getState();
  return queue.items.find((i) => i.uid === queue.currentUid);
}

let initialized = false;

export async function initPlayer() {
  if (initialized) return;
  initialized = true;
  const st = usePlayer.getState();
  await listen<PlayerStatus>("player:status", (e) => usePlayer.getState().setStatus(e.payload));
  await listen<QueueSnapshot>("queue:changed", (e) => usePlayer.getState().setQueue(e.payload));
  await listen<string>("player:error", (e) => usePlayer.getState().pushToast("error", e.payload));
  await listen<Settings>("settings:changed", (e) => usePlayer.getState().setSettings(e.payload));
  await listen("player:track", async () => {
    // Track boundaries move the queue's current pointer; refresh it.
    usePlayer.getState().setQueue(await api.getQueue());
  });
  const [status, queue, settings] = await Promise.all([api.getStatus(), api.getQueue(), api.getSettings()]);
  st.setStatus(status);
  st.setQueue(queue);
  st.setSettings(settings);
}
