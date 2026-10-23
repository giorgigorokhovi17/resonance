import { coverUrl } from "../components/Cover";

/**
 * A few display-friendly colours from a cover, for gradients (the Dynamic Island). Pixels are
 * grouped by hue; the biggest vivid groups win, and each colour is toned for white text on top.
 */
export type Palette = [string, string, string];

const cache = new Map<string, Palette | null>();
const inflight = new Map<string, Promise<Palette | null>>();

function toHsl(r: number, g: number, b: number): [number, number, number] {
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

const tone = (h: number, s: number, l: number) =>
  `hsl(${h.toFixed(0)} ${(Math.min(0.8, s < 0.1 ? s : Math.max(0.4, s)) * 100).toFixed(0)}% ${(Math.min(0.46, Math.max(0.24, l)) * 100).toFixed(0)}%)`;

async function extract(coverId: string): Promise<Palette | null> {
  const url = coverUrl(coverId, "thumb");
  if (!url) return null;
  const img = new Image();
  img.crossOrigin = "anonymous";
  img.src = url;
  await img.decode();
  const N = 32;
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = N;
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  if (!ctx) return null;
  ctx.drawImage(img, 0, 0, N, N);
  const d = ctx.getImageData(0, 0, N, N).data;
  // 12 hue buckets plus one for greys.
  const buckets = Array.from({ length: 13 }, () => ({ r: 0, g: 0, b: 0, w: 0 }));
  for (let i = 0; i < d.length; i += 4) {
    const [h, s, l] = toHsl(d[i], d[i + 1], d[i + 2]);
    const grey = s < 0.12 || l < 0.08 || l > 0.94;
    const k = grey ? 12 : Math.floor(h / 30) % 12;
    const w = grey ? 0.15 : 0.2 + s * Math.max(0, 1 - Math.abs(l - 0.5) * 1.6);
    const bk = buckets[k];
    bk.r += d[i] * w;
    bk.g += d[i + 1] * w;
    bk.b += d[i + 2] * w;
    bk.w += w;
  }
  const ranked = buckets
    .map((b, i) => ({ ...b, i }))
    .filter((b) => b.w > 0)
    .sort((a, b) => b.w - a.w)
    .map((b) => toHsl(b.r / b.w, b.g / b.w, b.b / b.w));
  if (!ranked.length) return null;
  const [h0, s0, l0] = ranked[0];
  // Secondary colours: the next distinct hues, or shifts of the main one for one-colour covers.
  const c1 = ranked[1] ?? [(h0 + 25) % 360, s0, l0 * 0.8];
  const c2 = ranked[2] ?? [(h0 + 340) % 360, s0, l0 * 0.65];
  return [tone(h0, s0, l0), tone(c1[0], c1[1], c1[2] * 0.9), tone(c2[0], c2[1], c2[2] * 0.75)];
}

export function coverPalette(coverId: string): Promise<Palette | null> {
  if (cache.has(coverId)) return Promise.resolve(cache.get(coverId)!);
  let p = inflight.get(coverId);
  if (!p) {
    p = extract(coverId)
      .catch(() => null)
      .then((v) => {
        cache.set(coverId, v);
        inflight.delete(coverId);
        return v;
      });
    inflight.set(coverId, p);
  }
  return p;
}
