import { create } from "zustand";

export type View =
  | { kind: "songs" }
  | { kind: "tracks" }
  | { kind: "playlists" }
  | { kind: "playlist"; id: number }
  | { kind: "review"; id: number }
  | { kind: "folders"; path?: string }
  | { kind: "search"; q: string }
  | { kind: "queue" }
  | { kind: "eq" }
  | { kind: "sources" }
  | { kind: "settings" };

interface UiStore {
  view: View;
  back: View[];
  forward: View[];
  nowPlaying: boolean;
  queueOpen: boolean;
  mini: boolean;
  setView: (v: View, replace?: boolean) => void;
  goBack: () => void;
  goForward: () => void;
  toggleNowPlaying: () => void;
  setNowPlaying: (v: boolean) => void;
  toggleQueue: () => void;
  setMini: (v: boolean) => void;
}

const same = (a: View, b: View) => JSON.stringify(a) === JSON.stringify(b);

export const useUi = create<UiStore>((set, get) => ({
  view: { kind: "songs" },
  back: [],
  forward: [],
  nowPlaying: false,
  queueOpen: false,
  mini: false,
  setView: (v, replace) => {
    const cur = get().view;
    if (same(cur, v)) {
      set({ nowPlaying: false });
      return;
    }
    if (replace) set({ view: v, nowPlaying: false });
    else set({ view: v, back: [...get().back.slice(-50), cur], forward: [], nowPlaying: false });
  },
  goBack: () => {
    const { back, view, forward } = get();
    if (!back.length) return;
    set({ view: back[back.length - 1], back: back.slice(0, -1), forward: [view, ...forward], nowPlaying: false });
  },
  goForward: () => {
    const { back, view, forward } = get();
    if (!forward.length) return;
    set({ view: forward[0], forward: forward.slice(1), back: [...back, view], nowPlaying: false });
  },
  toggleNowPlaying: () => set((s) => ({ nowPlaying: !s.nowPlaying })),
  setNowPlaying: (v) => set({ nowPlaying: v }),
  toggleQueue: () => set((s) => ({ queueOpen: !s.queueOpen })),
  setMini: (v) => set({ mini: v }),
}));
