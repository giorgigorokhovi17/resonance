import type { OutputFormat, SourceFormat, Track } from "./types";

export function fmtTime(secs: number): string {
  if (!isFinite(secs) || secs < 0) secs = 0;
  const s = Math.floor(secs);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

export function fmtDuration(secs: number): string {
  const total = Math.round(secs);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  if (h > 0) return `${h} hr ${m} min`;
  if (total < 60) return `${total} sec`;
  return `${m} min`;
}

export function fmtRate(hz: number): string {
  const k = hz / 1000;
  return `${Number.isInteger(k) ? k : k.toFixed(1)} kHz`;
}

export function fmtSource(s: SourceFormat | null): string {
  if (!s) return "";
  const parts = [s.codec];
  if (s.bitsPerSample && !s.float) parts.push(`${s.bitsPerSample}-bit`);
  parts.push(fmtRate(s.sampleRate));
  if (s.bitrateKbps) parts.push(`${s.bitrateKbps} kbps`);
  if (s.channels !== 2) parts.push(s.channels === 1 ? "mono" : `${s.channels} ch`);
  return parts.join(" · ");
}

export function fmtOutput(o: OutputFormat | null): string {
  if (!o) return "";
  const depth = o.float ? "32-bit float" : `${o.bits}-bit${o.containerBits !== o.bits ? ` (in ${o.containerBits})` : ""}`;
  return `${o.mode === "exclusive" ? "Exclusive" : "Shared"} · ${depth} · ${fmtRate(o.sampleRate)}`;
}

export function trackQuality(t: Track): string {
  const parts: string[] = [t.codec];
  if (t.bitDepth) parts.push(`${t.bitDepth}/${t.sampleRate ? +(t.sampleRate / 1000).toFixed(1) : "?"}`);
  else if (t.bitrate) parts.push(`${t.bitrate}k`);
  return parts.join(" ");
}

export function isHiRes(t: { bitDepth?: number; sampleRate?: number }): boolean {
  return (t.bitDepth ?? 0) >= 24 || (t.sampleRate ?? 0) > 48000;
}

export function displayArtist(t: Track): string {
  return t.artist || t.albumArtist || "Unknown Artist";
}

export function displayAlbum(t: Track): string {
  return t.album || "Unknown Album";
}
