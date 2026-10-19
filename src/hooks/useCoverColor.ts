import { useEffect, useState } from "react";
import { coverUrl } from "../components/Cover";

/**
 * Dominant, display-friendly color of a cover image (Spotify-style ambient backgrounds).
 * Sampled at 24x24 on a canvas; vivid mid-tone pixels count most, near-black/white least.
 */
const cache = new Map<string, string | null>();
const inflight = new Map<string, Promise<string | null>>();

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

async function extract(coverId: string): Promise<string | null> {
  const url = coverUrl(coverId, "thumb");
  if (!url) return null;
  const img = new Image();
  img.crossOrigin = "anonymous";
  img.src = url;
  await img.decode();
  const N = 24;
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = N;
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  if (!ctx) return null;
  ctx.drawImage(img, 0, 0, N, N);
  const d = ctx.getImageData(0, 0, N, N).data;
  let r = 0,
    g = 0,
    b = 0,
    w = 0;
  for (let i = 0; i < d.length; i += 4) {
    const [, s, l] = toHsl(d[i], d[i + 1], d[i + 2]);
    const wt = 0.04 + s * s * Math.max(0, 1 - Math.abs(l - 0.5) * 1.7);
    r += d[i] * wt;
    g += d[i + 1] * wt;
    b += d[i + 2] * wt;
    w += wt;
  }
  if (!w) return null;
  const [h, s, l] = toHsl(r / w, g / w, b / w);
  // Normalize into a rich, readable backdrop tone.
  const sat = s < 0.08 ? s : Math.min(0.75, Math.max(0.35, s));
  const light = Math.min(0.42, Math.max(0.28, l));
  return `hsl(${h.toFixed(0)} ${(sat * 100).toFixed(0)}% ${(light * 100).toFixed(0)}%)`;
}

export function coverColor(coverId: string): Promise<string | null> {
  if (cache.has(coverId)) return Promise.resolve(cache.get(coverId)!);
  let p = inflight.get(coverId);
  if (!p) {
    p = extract(coverId)
      .catch(() => null)
      .then((c) => {
        cache.set(coverId, c);
        inflight.delete(coverId);
        return c;
      });
    inflight.set(coverId, p);
  }
  return p;
}

export function useCoverColor(coverId: string | null | undefined): string | null {
  const [color, setColor] = useState<string | null>(() => (coverId ? cache.get(coverId) ?? null : null));
  useEffect(() => {
    let alive = true;
    if (!coverId) {
      setColor(null);
      return;
    }
    coverColor(coverId).then((c) => alive && setColor(c));
    return () => {
      alive = false;
    };
  }, [coverId]);
  return color;
}
