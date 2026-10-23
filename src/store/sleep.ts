import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { api } from "../lib/api";
import type { SleepStatus } from "../lib/types";

interface SleepStore {
  status: SleepStatus;
  /** performance.now() when `status` arrived, to count down between updates. */
  at: number;
}

export const useSleep = create<SleepStore>(() => ({ status: { mode: "off", remaining: null }, at: 0 }));

const set = (status: SleepStatus) => useSleep.setState({ status, at: performance.now() });

let initialized = false;

export async function initSleep() {
  if (initialized) return;
  initialized = true;
  await listen<SleepStatus>("sleep:changed", (e) => set(e.payload));
  set(await api.getSleepTimer());
}

export async function setSleepTimer(minutes: number | null, endOfSong = false) {
  set(await api.setSleepTimer(minutes, endOfSong));
}

/** Seconds left on a minutes timer, counted down locally. */
export function sleepRemaining(): number | null {
  const { status, at } = useSleep.getState();
  if (status.mode !== "minutes" || status.remaining == null) return null;
  return Math.max(0, status.remaining - (performance.now() - at) / 1000);
}
