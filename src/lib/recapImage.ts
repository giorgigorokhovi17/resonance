import { coverUrl } from "../components/Cover";
import { hm } from "../components/StatsCharts";
import type { Stats } from "./types";

interface SaveFilePickerOptions {
  suggestedName?: string;
  types?: { description?: string; accept: Record<string, string[]> }[];
}

interface FileSystemWritable {
  write: (data: Blob) => Promise<void>;
  close: () => Promise<void>;
}

interface FileSystemFileHandleLike {
  createWritable: () => Promise<FileSystemWritable>;
}

type WindowWithPicker = Window & {
  showSaveFilePicker?: (opts?: SaveFilePickerOptions) => Promise<FileSystemFileHandleLike>;
};

export interface RecapCardData {
  title: string;
  stats: Stats;
  coverId: string | null;
  coverColor: string | null;
  listenerLabel: string;
}

function cssVar(style: CSSStyleDeclaration, name: string, fallback: string) {
  return style.getPropertyValue(name).trim() || fallback;
}

function roundRectPath(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  const rad = Math.min(r, w / 2, h / 2);
  ctx.beginPath();
  ctx.moveTo(x + rad, y);
  ctx.arcTo(x + w, y, x + w, y + h, rad);
  ctx.arcTo(x + w, y + h, x, y + h, rad);
  ctx.arcTo(x, y + h, x, y, rad);
  ctx.arcTo(x, y, x + w, y, rad);
  ctx.closePath();
}

function drawTruncated(ctx: CanvasRenderingContext2D, text: string, x: number, y: number, maxWidth: number) {
  if (ctx.measureText(text).width <= maxWidth) {
    ctx.fillText(text, x, y);
    return;
  }
  const ellipsis = "…";
  let lo = 0;
  let hi = text.length;
  while (lo < hi) {
    const mid = Math.ceil((lo + hi) / 2);
    if (ctx.measureText(text.slice(0, mid) + ellipsis).width <= maxWidth) lo = mid;
    else hi = mid - 1;
  }
  ctx.fillText(text.slice(0, lo) + ellipsis, x, y);
}

async function loadImage(url: string): Promise<HTMLImageElement | null> {
  return new Promise((resolve) => {
    const img = new Image();
    img.crossOrigin = "anonymous";
    img.onload = () => resolve(img);
    img.onerror = () => resolve(null);
    img.src = url;
  });
}

function hoursText(secs: number) {
  const h = secs / 3600;
  return h >= 10 ? `${Math.round(h)} h` : h >= 1 ? `${h.toFixed(1)} h` : `${Math.round(secs / 60)} min`;
}

/** Renders the Recap summary card to a 1080×1350 PNG canvas using the active theme's colours. */
export async function renderRecapCanvas(data: RecapCardData): Promise<HTMLCanvasElement> {
  const W = 1080;
  const H = 1350;
  const canvas = document.createElement("canvas");
  canvas.width = W;
  canvas.height = H;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("Canvas is unavailable.");

  const cs = getComputedStyle(document.documentElement);
  const bg = cssVar(cs, "--c-bg", "#07080b");
  const panel = cssVar(cs, "--c-panel", "#1b1d24");
  const line = cssVar(cs, "--c-line", "#23252e");
  const fg = cssVar(cs, "--c-fg", "#e9eaf0");
  const muted = cssVar(cs, "--c-muted", "#9097a8");
  const faint = cssVar(cs, "--c-faint", "#5d6374");
  const accent = cssVar(cs, "--c-accent", "#f2a65e");
  const accent2 = cssVar(cs, "--c-accent2", "#eb6f92");
  const displayFont = `"Segoe UI Variable Display", "Segoe UI", system-ui, sans-serif`;
  const sansFont = `"Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif`;

  // Base background
  ctx.fillStyle = bg;
  ctx.fillRect(0, 0, W, H);

  // Ambient glow from cover colour or accent
  const glow = ctx.createRadialGradient(W * 0.5, H * 0.14, 40, W * 0.5, H * 0.18, W * 0.85);
  glow.addColorStop(0, data.coverColor ?? accent);
  glow.addColorStop(1, "transparent");
  ctx.save();
  ctx.globalAlpha = 0.28;
  ctx.fillStyle = glow;
  ctx.fillRect(0, 0, W, H);
  ctx.restore();

  const pad = 72;
  const innerW = W - pad * 2;

  // Header
  ctx.fillStyle = accent;
  ctx.font = `600 22px ${sansFont}`;
  ctx.fillText("UNDERTONE RECAP", pad, 108);

  ctx.fillStyle = fg;
  ctx.font = `700 58px ${displayFont}`;
  drawTruncated(ctx, data.title, pad, 178, innerW);

  // Hero card: top artwork + top artist / song / album
  const heroY = 230;
  const heroH = 310;
  roundRectPath(ctx, pad, heroY, innerW, heroH, 28);
  ctx.fillStyle = panel;
  ctx.fill();
  ctx.strokeStyle = line;
  ctx.lineWidth = 2;
  ctx.stroke();

  const artSize = 238;
  const artX = pad + 36;
  const artY = heroY + (heroH - artSize) / 2;

  let drewArt = false;
  const url = coverUrl(data.coverId, "full") ?? coverUrl(data.coverId, "thumb");
  if (url) {
    try {
      const img = await loadImage(url);
      if (img) {
        ctx.save();
        roundRectPath(ctx, artX, artY, artSize, artSize, 20);
        ctx.clip();
        ctx.drawImage(img, artX, artY, artSize, artSize);
        ctx.restore();
        drewArt = true;
      }
    } catch {
      // Ignore cover load failure; fall back to placeholder.
    }
  }
  if (!drewArt) {
    const grad = ctx.createLinearGradient(artX, artY, artX + artSize, artY + artSize);
    grad.addColorStop(0, accent);
    grad.addColorStop(1, accent2);
    roundRectPath(ctx, artX, artY, artSize, artSize, 20);
    ctx.fillStyle = grad;
    ctx.fill();
  }

  const metaX = artX + artSize + 40;
  const metaW = pad + innerW - 36 - metaX;
  const topArtist = data.stats.topArtists[0];
  const topSong = data.stats.topSongs[0];
  const topAlbum = data.stats.topAlbums[0];

  const rows: { label: string; value: string }[] = [];
  if (topArtist) rows.push({ label: "TOP ARTIST", value: topArtist.name });
  if (topSong) rows.push({ label: "TOP SONG", value: topSong.title });
  if (topAlbum) rows.push({ label: "TOP ALBUM", value: topAlbum.album });

  let curY = heroY + 62;
  for (const row of rows) {
    ctx.fillStyle = muted;
    ctx.font = `600 18px ${sansFont}`;
    ctx.fillText(row.label, metaX, curY);
    ctx.fillStyle = fg;
    ctx.font = `700 32px ${displayFont}`;
    drawTruncated(ctx, row.value, metaX, curY + 38, metaW);
    curY += 84;
  }

  // Five headline numbers
  const minutes = Math.round(data.stats.seconds / 60);
  const headlines = [
    { label: "Minutes listened", value: minutes.toLocaleString(), sub: hoursText(data.stats.seconds) },
    { label: "Plays", value: data.stats.plays.toLocaleString(), sub: "total plays" },
    { label: "Songs heard", value: data.stats.songs.toLocaleString(), sub: "different tracks" },
    { label: "Artists heard", value: data.stats.artists.toLocaleString(), sub: `${data.stats.albums.toLocaleString()} albums` },
    {
      label: "Longest streak",
      value: `${data.stats.longestStreak} ${data.stats.longestStreak === 1 ? "day" : "days"}`,
      sub: data.stats.topDay ? `Best day ${hm(data.stats.topDay[1])}` : data.listenerLabel,
    },
  ];

  // Top row: 2 wide tiles (Minutes, Plays)
  const gap = 24;
  const row1Y = heroY + heroH + 34;
  const tileH1 = 196;
  const tileW2 = (innerW - gap) / 2;

  for (let i = 0; i < 2; i++) {
    const item = headlines[i];
    const x = pad + i * (tileW2 + gap);
    roundRectPath(ctx, x, row1Y, tileW2, tileH1, 24);
    ctx.fillStyle = panel;
    ctx.fill();
    ctx.strokeStyle = line;
    ctx.lineWidth = 2;
    ctx.stroke();

    ctx.fillStyle = muted;
    ctx.font = `500 22px ${sansFont}`;
    ctx.fillText(item.label, x + 32, row1Y + 52);

    ctx.fillStyle = i === 0 ? accent : fg;
    ctx.font = `700 62px ${displayFont}`;
    drawTruncated(ctx, item.value, x + 32, row1Y + 124, tileW2 - 64);

    ctx.fillStyle = faint;
    ctx.font = `500 20px ${sansFont}`;
    drawTruncated(ctx, item.sub, x + 32, row1Y + 164, tileW2 - 64);
  }

  // Second row: 3 tiles (Songs, Artists, Longest streak)
  const row2Y = row1Y + tileH1 + gap;
  const tileH2 = 186;
  const tileW3 = (innerW - gap * 2) / 3;

  for (let i = 0; i < 3; i++) {
    const item = headlines[i + 2];
    const x = pad + i * (tileW3 + gap);
    roundRectPath(ctx, x, row2Y, tileW3, tileH2, 24);
    ctx.fillStyle = panel;
    ctx.fill();
    ctx.strokeStyle = line;
    ctx.lineWidth = 2;
    ctx.stroke();

    ctx.fillStyle = muted;
    ctx.font = `500 20px ${sansFont}`;
    ctx.fillText(item.label, x + 28, row2Y + 48);

    ctx.fillStyle = fg;
    ctx.font = `700 44px ${displayFont}`;
    drawTruncated(ctx, item.value, x + 28, row2Y + 108, tileW3 - 56);

    ctx.fillStyle = faint;
    ctx.font = `500 18px ${sansFont}`;
    drawTruncated(ctx, item.sub, x + 28, row2Y + 148, tileW3 - 56);
  }

  // Top songs strip if available
  const top3 = data.stats.topSongs.slice(0, 3);
  if (top3.length > 0) {
    const listY = row2Y + tileH2 + 34;
    const listH = 210;
    roundRectPath(ctx, pad, listY, innerW, listH, 24);
    ctx.fillStyle = panel;
    ctx.fill();
    ctx.strokeStyle = line;
    ctx.lineWidth = 2;
    ctx.stroke();

    ctx.fillStyle = muted;
    ctx.font = `600 18px ${sansFont}`;
    ctx.fillText("TOP TRACKS", pad + 32, listY + 44);

    top3.forEach((s, idx) => {
      const y = listY + 90 + idx * 46;
      ctx.fillStyle = accent;
      ctx.font = `700 24px ${displayFont}`;
      ctx.fillText(`${idx + 1}`, pad + 32, y);

      ctx.fillStyle = fg;
      ctx.font = `600 24px ${sansFont}`;
      drawTruncated(ctx, `${s.title} — ${s.artist}`, pad + 68, y, innerW - 220);

      ctx.fillStyle = muted;
      ctx.font = `500 20px ${sansFont}`;
      const plays = `${s.plays} ${s.plays === 1 ? "play" : "plays"}`;
      const pw = ctx.measureText(plays).width;
      ctx.fillText(plays, pad + innerW - 32 - pw, y);
    });
  }

  // Footer
  const gradBar = ctx.createLinearGradient(pad, 0, pad + 120, 0);
  gradBar.addColorStop(0, accent);
  gradBar.addColorStop(1, accent2);
  roundRectPath(ctx, pad, H - 96, 120, 6, 3);
  ctx.fillStyle = gradBar;
  ctx.fill();

  ctx.fillStyle = muted;
  ctx.font = `600 22px ${sansFont}`;
  ctx.fillText(data.listenerLabel, pad, H - 56);

  const brand = "Undertone";
  ctx.fillStyle = faint;
  ctx.font = `600 22px ${displayFont}`;
  const bw = ctx.measureText(brand).width;
  ctx.fillText(brand, pad + innerW - bw, H - 56);

  return canvas;
}

/** Saves a canvas as a PNG using the File System Access picker when available, or a download link. */
export async function saveRecapImage(data: RecapCardData, filename: string): Promise<"saved" | "downloaded" | "cancelled"> {
  const canvas = await renderRecapCanvas(data);
  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/png"));
  if (!blob) throw new Error("Couldn't encode image.");

  const picker = (window as WindowWithPicker).showSaveFilePicker;
  if (typeof picker === "function") {
    try {
      const handle = await picker({
        suggestedName: filename,
        types: [{ description: "PNG image", accept: { "image/png": [".png"] } }],
      });
      const writable = await handle.createWritable();
      await writable.write(blob);
      await writable.close();
      return "saved";
    } catch (e) {
      if (e instanceof DOMException && e.name === "AbortError") return "cancelled";
      // Fall through to browser download if the picker isn't supported in this context.
    }
  }

  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
  return "downloaded";
}
