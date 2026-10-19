import type { Appearance } from "./types";

/** Colour tokens of one theme variant. Each maps to a `--c-*` CSS variable (see index.css). */
export interface Palette {
  bg: string;
  elev: string;
  panel: string;
  line: string;
  fg: string;
  muted: string;
  faint: string;
  accent: string;
  accent2: string;
}

export interface Preset {
  id: string;
  name: string;
  dark: Palette;
  light: Palette;
}

export const PRESETS: Preset[] = [
  {
    id: "ember",
    name: "Ember",
    dark: { bg: "#07080b", elev: "#121318", panel: "#1b1d24", line: "#23252e", fg: "#e9eaf0", muted: "#9097a8", faint: "#5d6374", accent: "#f2a65e", accent2: "#eb6f92" },
    light: { bg: "#e9ebf0", elev: "#ffffff", panel: "#ffffff", line: "#e3e5eb", fg: "#15171d", muted: "#626879", faint: "#9a9fae", accent: "#bb5c25", accent2: "#c6466d" },
  },
  {
    id: "ocean",
    name: "Ocean",
    dark: { bg: "#060a12", elev: "#0f1622", panel: "#172030", line: "#1f2a3c", fg: "#e6edf7", muted: "#8b9ab3", faint: "#56647c", accent: "#5aa9ff", accent2: "#3fd0c9" },
    light: { bg: "#e6edf5", elev: "#ffffff", panel: "#ffffff", line: "#dbe3ee", fg: "#0f1724", muted: "#5a6880", faint: "#95a2b6", accent: "#1f6fd1", accent2: "#0e847f" },
  },
  {
    id: "aurora",
    name: "Aurora",
    dark: { bg: "#050b09", elev: "#0f1714", panel: "#16201c", line: "#1f2b26", fg: "#e5f0eb", muted: "#8aa399", faint: "#566b63", accent: "#4ade9b", accent2: "#38bdf8" },
    light: { bg: "#e7efeb", elev: "#ffffff", panel: "#ffffff", line: "#dce6e1", fg: "#0f1a16", muted: "#577068", faint: "#93a8a0", accent: "#138753", accent2: "#0b7cac" },
  },
  {
    id: "amethyst",
    name: "Amethyst",
    dark: { bg: "#09070f", elev: "#141120", panel: "#1c182b", line: "#262138", fg: "#ece9f5", muted: "#9a93b0", faint: "#635c78", accent: "#b18cff", accent2: "#f472d0" },
    light: { bg: "#ece9f3", elev: "#ffffff", panel: "#ffffff", line: "#e3dff0", fg: "#17141f", muted: "#66607a", faint: "#a09ab3", accent: "#7c4ddb", accent2: "#c0369a" },
  },
  {
    id: "rose",
    name: "Rosé",
    dark: { bg: "#0d0809", elev: "#1a1214", panel: "#24191c", line: "#2f2226", fg: "#f5e9ec", muted: "#b0959c", faint: "#775f66", accent: "#ff8fab", accent2: "#ffb38a" },
    light: { bg: "#f5ebec", elev: "#fffafa", panel: "#ffffff", line: "#efdfe2", fg: "#22161a", muted: "#7a6066", faint: "#b39ba1", accent: "#c93d67", accent2: "#b45f30" },
  },
  {
    id: "nord",
    name: "Nord",
    dark: { bg: "#1f232b", elev: "#272c36", panel: "#2e3440", line: "#3b4252", fg: "#eceff4", muted: "#a3acbd", faint: "#6d778a", accent: "#88c0d0", accent2: "#b48ead" },
    light: { bg: "#e5e9f0", elev: "#f6f8fb", panel: "#ffffff", line: "#d8dee9", fg: "#2e3440", muted: "#5c6577", faint: "#98a1b3", accent: "#3b7c92", accent2: "#8a5f84" },
  },
  {
    id: "sand",
    name: "Sand",
    dark: { bg: "#0c0a07", elev: "#181510", panel: "#211d16", line: "#2c271e", fg: "#f0ebe1", muted: "#a89f8d", faint: "#6f6757", accent: "#e0b45c", accent2: "#d9805a" },
    light: { bg: "#efe9de", elev: "#fbf8f2", panel: "#fffdf8", line: "#e4dccd", fg: "#1f1a12", muted: "#6e6553", faint: "#a69c88", accent: "#9a6a12", accent2: "#b5532a" },
  },
  {
    id: "mono",
    name: "Mono",
    dark: { bg: "#080808", elev: "#121212", panel: "#1b1b1b", line: "#262626", fg: "#ededed", muted: "#9a9a9a", faint: "#5f5f5f", accent: "#e6e6e6", accent2: "#a3a3a3" },
    light: { bg: "#ebebeb", elev: "#ffffff", panel: "#ffffff", line: "#e2e2e2", fg: "#161616", muted: "#646464", faint: "#9e9e9e", accent: "#2a2a2a", accent2: "#6b6b6b" },
  },
];

/** Gradient pairs offered as one-click accent swatches. */
export const ACCENT_SWATCHES: [string, string][] = [
  ["#ff7a59", "#ff3d7f"],
  ["#f59e0b", "#ef4444"],
  ["#eab308", "#84cc16"],
  ["#22c55e", "#14b8a6"],
  ["#06b6d4", "#3b82f6"],
  ["#5aa9ff", "#7c5cff"],
  ["#a855f7", "#ec4899"],
  ["#e11d48", "#f97316"],
];

export const DEFAULT_APPEARANCE: Appearance = {
  preset: "ember",
  accent: null,
  accent2: null,
  coverAccent: false,
  pureBlack: false,
  ambient: true,
  ambientIntensity: 1,
  backdropBlur: 90,
  corners: "default",
  uiScale: 1,
};

export const CORNER_SCALE: Record<Appearance["corners"], number> = { sharp: 0.35, default: 1, round: 1.6 };

// ---- colour maths -------------------------------------------------------------------------------

type Rgb = [number, number, number];

/** Parses `#rgb`, `#rrggbb`, `rgb(...)` and `hsl(...)` (comma or space separated). */
export function parseColor(c: string): Rgb | null {
  const s = c.trim().toLowerCase();
  let m = /^#([0-9a-f]{3}|[0-9a-f]{6})$/.exec(s);
  if (m) {
    const h = m[1].length === 3 ? [...m[1]].map((x) => x + x).join("") : m[1];
    return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16)) as Rgb;
  }
  m = /^(rgb|hsl)a?\(([^)]*)\)$/.exec(s);
  if (!m) return null;
  const n = m[2].split(/[\s,/]+/).filter(Boolean).map((x) => parseFloat(x));
  if (n.length < 3 || n.some((x) => Number.isNaN(x))) return null;
  if (m[1] === "rgb") return [n[0], n[1], n[2]];
  return hslToRgb(n[0], n[1] / 100, n[2] / 100);
}

export function toHex([r, g, b]: Rgb): string {
  return "#" + [r, g, b].map((x) => Math.round(Math.min(255, Math.max(0, x))).toString(16).padStart(2, "0")).join("");
}

function rgbToHsl([r, g, b]: Rgb): [number, number, number] {
  r /= 255;
  g /= 255;
  b /= 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  if (max === min) return [0, 0, l];
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  let h = 0;
  if (max === r) h = (g - b) / d + (g < b ? 6 : 0);
  else if (max === g) h = (b - r) / d + 2;
  else h = (r - g) / d + 4;
  return [h * 60, s, l];
}

function hslToRgb(h: number, s: number, l: number): Rgb {
  h = ((h % 360) + 360) % 360;
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = l - c / 2;
  const [r, g, b] = h < 60 ? [c, x, 0] : h < 120 ? [x, c, 0] : h < 180 ? [0, c, x] : h < 240 ? [0, x, c] : h < 300 ? [x, 0, c] : [c, 0, x];
  return [(r + m) * 255, (g + m) * 255, (b + m) * 255];
}

function luminance([r, g, b]: Rgb): number {
  const ch = (v: number) => {
    v /= 255;
    return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b);
}

/** WCAG contrast ratio, 1..21. */
export function contrast(a: Rgb, b: Rgb): number {
  const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
}

function mix(a: Rgb, b: Rgb, t: number): Rgb {
  return [0, 1, 2].map((i) => a[i] + (b[i] - a[i]) * t) as Rgb;
}

/** Moves the lightness of `c` away from `bg` until it reaches `min` contrast. */
function ensureContrast(c: Rgb, bg: Rgb, min: number): Rgb {
  if (contrast(c, bg) >= min) return c;
  const [h, s, l] = rgbToHsl(c);
  const up = luminance(bg) < 0.5;
  for (let step = 1; step <= 50; step++) {
    const nl = up ? l + (1 - l) * (step / 50) : l * (1 - step / 50);
    const out = hslToRgb(h, s, nl);
    if (contrast(out, bg) >= min) return out;
  }
  return up ? [255, 255, 255] : [0, 0, 0];
}

const WHITE: Rgb = [255, 255, 255];
const INK: Rgb = [17, 17, 20];

/** Minimum contrast for the accent against the content background (icons, active text, progress). */
export const ACCENT_MIN_CONTRAST = 3;
/** Minimum contrast for button text on the accent. */
export const BUTTON_MIN_CONTRAST = 4.5;

export interface ResolvedTheme {
  vars: Record<string, string>;
  /** Accent against the content background. */
  accentContrast: number;
  /** Button text against the weaker of the two accent colours. */
  buttonContrast: number;
  /** True when a custom or album-art accent had to be lightened or darkened to stay readable. */
  adjusted: boolean;
}

export function presetById(id: string): Preset {
  return PRESETS.find((p) => p.id === id) ?? PRESETS[0];
}

/** Turns stored appearance settings into concrete CSS variables for one mode. */
export function resolveTheme(a: Appearance, dark: boolean, coverColor: string | null): ResolvedTheme {
  const p: Palette = { ...(dark ? presetById(a.preset).dark : presetById(a.preset).light) };
  const rgb = (c: string) => parseColor(c) ?? [128, 128, 128];

  if (dark && a.pureBlack) {
    p.bg = "#000000";
    p.elev = toHex(mix(rgb(p.elev), [0, 0, 0], 0.5));
    p.panel = toHex(mix(rgb(p.panel), [0, 0, 0], 0.3));
    p.line = toHex(mix(rgb(p.line), [0, 0, 0], 0.2));
  }

  let accent = rgb(p.accent);
  let accent2 = rgb(p.accent2);
  let custom = false;
  const cover = a.coverAccent && coverColor ? parseColor(coverColor) : null;
  if (cover) {
    const [h, s] = rgbToHsl(cover);
    const sat = s < 0.08 ? s : Math.min(0.85, Math.max(0.5, s));
    accent = hslToRgb(h, sat, dark ? 0.68 : 0.42);
    accent2 = hslToRgb(h + 40, sat, dark ? 0.64 : 0.46);
    custom = true;
  } else if (a.accent && parseColor(a.accent)) {
    accent = rgb(a.accent);
    if (a.accent2 && parseColor(a.accent2)) {
      accent2 = rgb(a.accent2);
    } else {
      const [h, s, l] = rgbToHsl(accent);
      accent2 = hslToRgb(h + 35, s, l);
    }
    custom = true;
  }

  // Accent fills carry white text in light mode and near-black text in dark mode. Lightness is
  // pushed away from the background until the accent reads against it and the text reads on it.
  const surface = rgb(p.elev);
  const onAccent = dark ? INK : WHITE;
  const fix = (c: Rgb) => ensureContrast(ensureContrast(c, surface, ACCENT_MIN_CONTRAST), onAccent, BUTTON_MIN_CONTRAST);
  const fixedA = fix(accent);
  const fixedB = fix(accent2);
  const adjusted = custom && (toHex(fixedA) !== toHex(accent) || toHex(fixedB) !== toHex(accent2));
  accent = fixedA;
  accent2 = fixedB;

  const fg = rgb(p.fg);
  const vars: Record<string, string> = {
    "--c-bg": p.bg,
    "--c-elev": p.elev,
    "--c-panel": p.panel,
    "--c-hover": `rgba(${fg.map(Math.round).join(", ")}, ${dark ? 0.07 : 0.055})`,
    "--c-line": p.line,
    "--c-fg": p.fg,
    "--c-muted": p.muted,
    "--c-faint": p.faint,
    "--c-accent": toHex(accent),
    "--c-accent2": toHex(accent2),
    "--c-on-accent": toHex(onAccent),
  };
  return {
    vars,
    accentContrast: contrast(accent, surface),
    buttonContrast: Math.min(contrast(onAccent, accent), contrast(onAccent, accent2)),
    adjusted,
  };
}
