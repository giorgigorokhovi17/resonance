import type { IslandOptions, IslandPosition } from "./types";
import type { Palette } from "./palette";

/** Built-in island colours (the "From the cover" option is computed per song). */
export const ISLAND_PALETTES: { id: string; label: string; p: Palette }[] = [
  { id: "midnight", label: "Midnight", p: ["hsl(250 35% 30%)", "hsl(280 35% 26%)", "hsl(220 35% 22%)"] },
  { id: "sunset", label: "Sunset", p: ["hsl(12 70% 42%)", "hsl(330 55% 38%)", "hsl(40 75% 40%)"] },
  { id: "ocean", label: "Ocean", p: ["hsl(200 70% 32%)", "hsl(180 60% 28%)", "hsl(225 60% 30%)"] },
  { id: "forest", label: "Forest", p: ["hsl(140 40% 26%)", "hsl(95 35% 28%)", "hsl(170 40% 22%)"] },
  { id: "candy", label: "Candy", p: ["hsl(320 60% 42%)", "hsl(270 55% 45%)", "hsl(200 65% 42%)"] },
  { id: "mono", label: "Graphite", p: ["hsl(0 0% 16%)", "hsl(0 0% 24%)", "hsl(0 0% 10%)"] },
];

/** The colours the island uses when nothing else applies. */
export const FALLBACK_PALETTE: Palette = ISLAND_PALETTES[0].p;

/** A fixed palette for these options, or null for "From the cover". */
export function fixedPalette(o: IslandOptions): Palette | null {
  if (o.colors === "custom" && o.customColors.length === 3) return o.customColors as Palette;
  return ISLAND_PALETTES.find((x) => x.id === o.colors)?.p ?? null;
}

/** The 3×3 picker grid, row by row (the middle cell is empty). */
export const POSITION_GRID: (IslandPosition | null)[] = [
  "topLeft",
  "topCenter",
  "topRight",
  "middleLeft",
  null,
  "middleRight",
  "bottomLeft",
  "bottomCenter",
  "bottomRight",
];

export const POSITION_LABELS: Record<IslandPosition, string> = {
  topLeft: "Top left",
  topCenter: "Top centre",
  topRight: "Top right",
  middleLeft: "Middle left",
  middleRight: "Middle right",
  bottomLeft: "Bottom left",
  bottomCenter: "Bottom centre",
  bottomRight: "Bottom right",
  custom: "Where I dragged it",
};

export type Rect = { x: number; y: number; w: number; h: number };
type Size = { w: number; h: number };

/** Space between the island and the screen edge. */
const MARGIN = 8;

const clamp = (v: number, lo: number, hi: number) => Math.min(Math.max(v, lo), Math.max(lo, hi));

/** The screen area (outside the taskbar) holding the point, or the first one. */
export function areaAt(areas: Rect[], x: number, y: number): Rect | null {
  return areas.find((a) => x >= a.x && x < a.x + a.w && y >= a.y && y < a.y + a.h) ?? areas[0] ?? null;
}

/** Where the collapsed island goes: a corner or edge of `area`, or the custom spot kept on screen. */
export function collapsedRect(o: IslandOptions, area: Rect, size: Size): Rect {
  if (o.position === "custom" && o.x != null && o.y != null) {
    return { x: clamp(o.x, area.x, area.x + area.w - size.w), y: clamp(o.y, area.y, area.y + area.h - size.h), ...size };
  }
  const p = o.position === "custom" ? "topCenter" : o.position;
  const x = p.endsWith("Left") ? area.x + MARGIN : p.endsWith("Right") ? area.x + area.w - size.w - MARGIN : area.x + (area.w - size.w) / 2;
  const y = p.startsWith("top") ? area.y + MARGIN : p.startsWith("bottom") ? area.y + area.h - size.h - MARGIN : area.y + (area.h - size.h) / 2;
  return { x: Math.round(x), y: Math.round(y), ...size };
}

/**
 * The opened island grows from the collapsed one: centred on it, downwards in the top half of
 * the screen and upwards in the bottom half, and kept on screen. `up` says which way it grew.
 */
export function grownRect(small: Rect, size: Size, area: Rect): { rect: Rect; up: boolean } {
  const up = small.y + small.h / 2 > area.y + area.h / 2;
  const x = clamp(small.x + small.w / 2 - size.w / 2, area.x, area.x + area.w - size.w);
  const y = clamp(up ? small.y + small.h - size.h : small.y, area.y, area.y + area.h - size.h);
  return { rect: { x: Math.round(x), y: Math.round(y), ...size }, up };
}
