import { create } from "zustand";

/** How the phone shows songs. Kept on the phone: it's about this screen, not the library. */
export interface ListStyle {
  size: "compact" | "normal" | "large";
  cover: boolean;
  artist: boolean;
  bold: boolean;
  /** The quality tag on each song. */
  quality: boolean;
  /** The quality tag on the mini player. */
  playerQuality: boolean;
  /** See-through bars and cards. */
  glass: boolean;
}

const KEY = "list.style";
const DEFAULTS: ListStyle = { size: "normal", cover: true, artist: true, bold: false, quality: false, playerQuality: true, glass: true };

function load(): ListStyle {
  try {
    return { ...DEFAULTS, ...JSON.parse(localStorage.getItem(KEY) ?? "{}") };
  } catch {
    return DEFAULTS;
  }
}

const applyGlass = (on: boolean) => document.documentElement.classList.toggle("no-glass", !on);

export const useListStyle = create<ListStyle & { update: (p: Partial<ListStyle>) => void }>((set, get) => ({
  ...load(),
  update: (p) => {
    set(p);
    const { update: _, ...style } = get();
    applyGlass(style.glass);
    try {
      localStorage.setItem(KEY, JSON.stringify(style));
    } catch {
      /* ignore */
    }
  },
}));

applyGlass(useListStyle.getState().glass);

/** Height of a song row. */
export function rowHeight(s: Pick<ListStyle, "size" | "artist">): number {
  const h = { compact: 52, normal: 64, large: 76 }[s.size];
  return s.artist ? h : h - 8;
}

export function useRowHeight(): number {
  return useListStyle((s) => rowHeight(s));
}
