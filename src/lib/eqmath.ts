import type { EqBand, EqSettings } from "./types";

/** RBJ biquad design + magnitude response, mirroring src-tauri/src/audio/eq.rs for live drawing. */
interface Coeffs {
  b0: number;
  b1: number;
  b2: number;
  a1: number;
  a2: number;
}

export function bandActive(b: EqBand): boolean {
  if (!b.enabled) return false;
  return b.filter === "lowPass" || b.filter === "highPass" || Math.abs(b.gain) > 1e-9;
}

export function design(b: EqBand, fs: number): Coeffs {
  const f0 = Math.max(1, Math.min(b.freq, fs * 0.49));
  const w0 = (2 * Math.PI * f0) / fs;
  const cs = Math.cos(w0);
  const sn = Math.sin(w0);
  const alpha = sn / (2 * Math.max(b.q, 0.01));
  const A = Math.pow(10, b.gain / 40);
  let b0, b1, b2, a0, a1, a2;
  switch (b.filter) {
    case "peaking":
      [b0, b1, b2, a0, a1, a2] = [1 + alpha * A, -2 * cs, 1 - alpha * A, 1 + alpha / A, -2 * cs, 1 - alpha / A];
      break;
    case "lowShelf": {
      const s = 2 * Math.sqrt(A) * alpha;
      [b0, b1, b2, a0, a1, a2] = [
        A * (A + 1 - (A - 1) * cs + s),
        2 * A * (A - 1 - (A + 1) * cs),
        A * (A + 1 - (A - 1) * cs - s),
        A + 1 + (A - 1) * cs + s,
        -2 * (A - 1 + (A + 1) * cs),
        A + 1 + (A - 1) * cs - s,
      ];
      break;
    }
    case "highShelf": {
      const s = 2 * Math.sqrt(A) * alpha;
      [b0, b1, b2, a0, a1, a2] = [
        A * (A + 1 + (A - 1) * cs + s),
        -2 * A * (A - 1 + (A + 1) * cs),
        A * (A + 1 + (A - 1) * cs - s),
        A + 1 - (A - 1) * cs + s,
        2 * (A - 1 - (A + 1) * cs),
        A + 1 - (A - 1) * cs - s,
      ];
      break;
    }
    case "lowPass":
      [b0, b1, b2, a0, a1, a2] = [(1 - cs) / 2, 1 - cs, (1 - cs) / 2, 1 + alpha, -2 * cs, 1 - alpha];
      break;
    case "highPass":
      [b0, b1, b2, a0, a1, a2] = [(1 + cs) / 2, -(1 + cs), (1 + cs) / 2, 1 + alpha, -2 * cs, 1 - alpha];
      break;
  }
  return { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 };
}

export function responseDb(c: Coeffs, f: number, fs: number): number {
  const w = (2 * Math.PI * f) / fs;
  const c1 = Math.cos(w),
    s1 = Math.sin(w),
    c2 = Math.cos(2 * w),
    s2 = Math.sin(2 * w);
  const nr = c.b0 + c.b1 * c1 + c.b2 * c2;
  const ni = -(c.b1 * s1 + c.b2 * s2);
  const dr = 1 + c.a1 * c1 + c.a2 * c2;
  const di = -(c.a1 * s1 + c.a2 * s2);
  return 10 * Math.log10(Math.max((nr * nr + ni * ni) / Math.max(dr * dr + di * di, 1e-300), 1e-300));
}

export const FS_DISPLAY = 48000;

/** Peak of the summed band response (no preamp). */
export function maxResponseDb(eq: EqSettings, fs = FS_DISPLAY): number {
  const filters = eq.bands.filter(bandActive).map((b) => design(b, fs));
  if (!filters.length) return 0;
  let peak = -Infinity;
  const hi = Math.min(fs * 0.49, 22000);
  for (let i = 0; i < 600; i++) {
    const f = 10 * Math.pow(hi / 10, i / 599);
    let db = 0;
    for (const c of filters) db += responseDb(c, f, fs);
    peak = Math.max(peak, db);
  }
  return peak;
}

export function effectivePreamp(eq: EqSettings, fs = FS_DISPLAY): number {
  if (!eq.enabled) return 0;
  if (!eq.autoHeadroom) return eq.preamp;
  const total = eq.preamp + maxResponseDb(eq, fs);
  return total > 0 ? eq.preamp - total : eq.preamp;
}

/** Curve points (log-spaced, 20 Hz–20 kHz) of the total response including effective preamp. */
export function curve(eq: EqSettings, points = 240, fs = FS_DISPLAY): { f: number; db: number }[] {
  const filters = eq.enabled ? eq.bands.filter(bandActive).map((b) => design(b, fs)) : [];
  const pre = effectivePreamp(eq, fs);
  return Array.from({ length: points }, (_, i) => {
    const f = 20 * Math.pow(1000, i / (points - 1));
    let db = pre;
    for (const c of filters) db += responseDb(c, f, fs);
    return { f, db };
  });
}

export const DEFAULT_FREQS = [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];

export function flatEq(enabled = true): EqSettings {
  return {
    enabled,
    preamp: 0,
    autoHeadroom: true,
    presetName: "Flat",
    bands: DEFAULT_FREQS.map((f, i) => ({
      enabled: true,
      filter: i === 0 ? "lowShelf" : i === 9 ? "highShelf" : "peaking",
      freq: f,
      gain: 0,
      q: i === 0 || i === 9 ? 0.707 : 1.41,
    })),
  };
}

function withGains(name: string, gains: number[]): EqSettings {
  const eq = flatEq(true);
  eq.presetName = name;
  eq.bands = eq.bands.map((b, i) => ({ ...b, gain: gains[i] ?? 0 }));
  return eq;
}

export const BUILTIN_PRESETS: EqSettings[] = [
  flatEq(true),
  withGains("Bass boost", [6, 4.5, 3, 1, 0, 0, 0, 0, 0, 0]),
  withGains("Treble boost", [0, 0, 0, 0, 0, 0.5, 1.5, 3, 4.5, 5]),
  withGains("Loudness", [5, 3.5, 1, 0, -1, -1, 0, 1, 3, 4]),
  withGains("Vocal", [-2, -1.5, -1, 1, 2.5, 3, 2.5, 1, 0, -1]),
  withGains("Warm", [2, 2, 1.5, 0.5, 0, 0, -0.5, -1, -2, -3]),
];
