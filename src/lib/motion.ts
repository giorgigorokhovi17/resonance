import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { usePlayer } from "../store/player";
import { useUi } from "../store/ui";
import type { AnimatedArt } from "./types";

/**
 * Decides which GIF and video artwork may move. Performance comes first in an audio player, so:
 * at most MAX_ACTIVE at once, only what is on screen, nothing while the window is hidden,
 * minimised or in the tray, and nothing when Windows asks for less motion. Everything else shows
 * its still poster.
 *
 * Where artwork sits decides when it moves:
 * - "nowPlaying", "app" (the app background) and "page" (a playlist page's header and
 *   background) move whenever they're visible;
 * - "tile" (grid and list tiles, the player bar) moves only while the pointer is over it;
 * - virtualised rows don't register at all and always show the poster.
 */
export type MotionPlace = "nowPlaying" | "app" | "page" | "tile";

const MAX_ACTIVE = 4;
const RANK: Record<MotionPlace, number> = { nowPlaying: 0, app: 1, page: 2, tile: 3 };

interface Slot {
  place: MotionPlace;
  visible: boolean;
  hovered: boolean;
  active: boolean;
  /** Newer (or more recently hovered) slots win ties. */
  order: number;
  set: (on: boolean) => void;
}

const slots = new Set<Slot>();
const byElement = new WeakMap<Element, Slot>();
let counter = 0;
let pageVisible = true;
let minimized = false;
/** Hidden to the tray. */
let hidden = false;
let reducedMotion = false;
let mode: AnimatedArt = "off";
let nowPlayingOpen = false;
let scheduled = false;
let observer: IntersectionObserver | null = null;

function allowed(s: Slot): boolean {
  if (!pageVisible || minimized || hidden || reducedMotion || mode === "off" || !s.visible) return false;
  if (s.place === "tile" && !s.hovered) return false;
  // Now Playing covers the library: only it and the app background behind it can be seen.
  const seen = s.place === "nowPlaying" || s.place === "app";
  if (nowPlayingOpen && !seen) return false;
  if (mode === "nowPlaying" && !(nowPlayingOpen && seen)) return false;
  return true;
}

function reconcile() {
  scheduled = false;
  const wanted = [...slots].filter(allowed).sort((a, b) => RANK[a.place] - RANK[b.place] || b.order - a.order);
  const on = new Set(wanted.slice(0, MAX_ACTIVE));
  for (const s of slots) {
    const active = on.has(s);
    if (active !== s.active) {
      s.active = active;
      s.set(active);
    }
  }
}

function schedule() {
  if (scheduled) return;
  scheduled = true;
  queueMicrotask(reconcile);
}

let started = false;
function start() {
  if (started) return;
  started = true;
  pageVisible = document.visibilityState === "visible";
  document.addEventListener("visibilitychange", () => {
    pageVisible = document.visibilityState === "visible";
    schedule();
  });
  const mq = window.matchMedia("(prefers-reduced-motion: reduce)");
  reducedMotion = mq.matches;
  mq.addEventListener("change", () => {
    reducedMotion = mq.matches;
    schedule();
  });
  const readMode = () => usePlayer.getState().settings?.appearance.animatedArt ?? "off";
  mode = readMode();
  usePlayer.subscribe(() => {
    const m = readMode();
    if (m !== mode) {
      mode = m;
      schedule();
    }
  });
  nowPlayingOpen = useUi.getState().nowPlaying;
  useUi.subscribe((s) => {
    if (s.nowPlaying !== nowPlayingOpen) {
      nowPlayingOpen = s.nowPlaying;
      schedule();
    }
  });
  // WebView2 keeps reporting "visible" while the window is minimised or hidden in the tray, so
  // ask the window itself, and listen for the backend hiding and showing it.
  const win = getCurrentWindow();
  const checkWindow = () =>
    Promise.all([win.isMinimized(), win.isVisible()])
      .then(([m, v]) => {
        if (m !== minimized || v === hidden) {
          minimized = m;
          hidden = !v;
          schedule();
        }
      })
      .catch(() => {});
  checkWindow();
  win.onResized(checkWindow).catch(() => {});
  win.onFocusChanged(checkWindow).catch(() => {});
  listen<boolean>("window:hidden", (e) => {
    hidden = e.payload;
    schedule();
  }).catch(() => {});
  observer = new IntersectionObserver((entries) => {
    for (const e of entries) {
      const s = byElement.get(e.target);
      if (s) s.visible = e.isIntersecting;
    }
    schedule();
  });
}

/**
 * Registers artwork that could move. Returns a ref for its element and whether it may play now.
 * With `enabled` false (a still picture), nothing is registered.
 */
export function useMotionSlot<T extends HTMLElement = HTMLDivElement>(place: MotionPlace, enabled: boolean) {
  const ref = useRef<T>(null);
  const [active, setActive] = useState(false);
  useEffect(() => {
    const el = ref.current;
    if (!enabled || !el) return;
    start();
    const s: Slot = { place, visible: false, hovered: false, active: false, order: ++counter, set: setActive };
    slots.add(s);
    byElement.set(el, s);
    observer?.observe(el);
    let hoverTarget: Element | null = null;
    const enter = () => {
      s.hovered = true;
      s.order = ++counter;
      schedule();
    };
    const leave = () => {
      s.hovered = false;
      schedule();
    };
    if (place === "tile") {
      // The whole card counts, not only the picture.
      hoverTarget = el.closest("[data-motion-hover]") ?? el;
      hoverTarget.addEventListener("pointerenter", enter);
      hoverTarget.addEventListener("pointerleave", leave);
      s.hovered = hoverTarget.matches(":hover");
    }
    schedule();
    return () => {
      slots.delete(s);
      observer?.unobserve(el);
      hoverTarget?.removeEventListener("pointerenter", enter);
      hoverTarget?.removeEventListener("pointerleave", leave);
      setActive(false);
      schedule();
    };
  }, [place, enabled]);
  return [ref, enabled && active] as const;
}

/** How many pieces of artwork are moving right now (for diagnostics). */
export function activeMotionCount(): number {
  return [...slots].filter((s) => s.active).length;
}
