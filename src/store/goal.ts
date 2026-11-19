import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { api } from "../lib/api";
import { usePlayer } from "./player";

const MET_DAY_KEY = "goal.metDay";

interface GoalStore {
  /** Seconds listened today (since local midnight). */
  seconds: number;
  loaded: boolean;
  /** Incremented once per day when the goal is newly met, to trigger the ring's glow pulse. */
  justMet: number;
}

export const useGoal = create<GoalStore>(() => ({ seconds: 0, loaded: false, justMet: 0 }));

/** Unix seconds at today's local midnight. */
export function localMidnight(): number {
  const d = new Date();
  d.setHours(0, 0, 0, 0);
  return Math.floor(d.getTime() / 1000);
}

function todayKey(): string {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

function readMetDay(): string | null {
  try {
    return localStorage.getItem(MET_DAY_KEY);
  } catch {
    return null;
  }
}

function writeMetDay(day: string) {
  try {
    localStorage.setItem(MET_DAY_KEY, day);
  } catch {
    // ignore storage errors
  }
}

function checkGoalMet(seconds: number, goalMin: number) {
  if (goalMin <= 0 || seconds < goalMin * 60) return;
  const today = todayKey();
  if (readMetDay() === today) return;
  writeMetDay(today);
  useGoal.setState((s) => ({ justMet: s.justMet + 1 }));
  usePlayer.getState().pushToast("info", "Daily goal met.");
}

let inFlight: Promise<void> | null = null;

/** Fetches today's listening time from local midnight to now, deduplicating concurrent calls. */
export function refreshGoal(): Promise<void> {
  const settings = usePlayer.getState().settings;
  if (settings && settings.listeningGoalMin <= 0) return Promise.resolve();
  if (inFlight) return inFlight;
  const from = localMidnight();
  const to = Math.floor(Date.now() / 1000) + 1;
  inFlight = api
    .getStats(from, to)
    .then((s) => {
      useGoal.setState({ seconds: s.seconds, loaded: true });
      const goalMin = usePlayer.getState().settings?.listeningGoalMin ?? 0;
      checkGoalMet(s.seconds, goalMin);
    })
    .catch(() => {})
    .finally(() => {
      inFlight = null;
    });
  return inFlight;
}

let initialized = false;

export async function initGoal() {
  if (initialized) return;
  initialized = true;

  let prevGoal = usePlayer.getState().settings?.listeningGoalMin ?? 0;
  usePlayer.subscribe((st) => {
    const goalMin = st.settings?.listeningGoalMin ?? 0;
    if (goalMin === prevGoal) return;
    const wasOff = prevGoal <= 0;
    prevGoal = goalMin;
    if (goalMin <= 0) return;
    if (wasOff || !useGoal.getState().loaded) {
      void refreshGoal();
    } else {
      checkGoalMet(useGoal.getState().seconds, goalMin);
    }
  });

  // Track changes record the finished play on a background thread; wait briefly before reading.
  let trackTimer = 0;
  await listen("player:track", () => {
    window.clearTimeout(trackTimer);
    trackTimer = window.setTimeout(() => void refreshGoal(), 300);
  });

  window.addEventListener("focus", () => void refreshGoal());
  window.setInterval(() => void refreshGoal(), 60_000);

  await refreshGoal();
}
