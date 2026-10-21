import { open } from "@tauri-apps/plugin-dialog";
import { api } from "./api";
import type { Media, MediaPurpose, StagedMedia } from "./types";
import { mediaUrl } from "../store/media";

/**
 * Importing custom artwork and backgrounds. The backend checks the file's format, codec,
 * extension and size and copies it into the app; for a GIF or video, this side then reads its
 * resolution and length from a hidden element, refuses anything over the limits, and makes the
 * still poster from the first frame (stored like any other cover).
 */

const PICTURES = ["jpg", "jpeg", "png", "webp"];
const VIDEOS = ["mp4", "m4v", "webm"];

/** Same limits as the backend (library/media.rs). */
const VIDEO_LIMITS: Record<MediaPurpose, { ms: number; longPx: number; shortPx: number }> = {
  cover: { ms: 10_000, longPx: 1080, shortPx: 1080 },
  background: { ms: 60_000, longPx: 1920, shortPx: 1080 },
};

const POSTER_PX = 1400;

export type PickKind = "any" | "photo" | "video";

/** Opens a file picker for artwork or a background. */
export async function pickMedia(title: string, kind: PickKind = "any"): Promise<string | null> {
  const photos = { name: "Pictures (JPG, PNG, WebP, GIF)", extensions: [...PICTURES, "gif"] };
  const videos = { name: "Videos (MP4 H.264, WebM VP9)", extensions: VIDEOS };
  const filters =
    kind === "photo"
      ? [photos]
      : kind === "video"
        ? [videos]
        : [{ name: "Pictures and videos", extensions: [...PICTURES, "gif", ...VIDEOS] }, photos, videos];
  const picked = await open({ multiple: false, title, filters });
  return picked && !Array.isArray(picked) ? picked : null;
}

export interface ImportedCover {
  /** Still cover (the poster for a GIF or video). */
  coverId: string;
  /** The GIF or video, or null for a still picture. */
  motionId: string | null;
}

/** Imports a file as a song or playlist cover. */
export async function importCover(path: string): Promise<ImportedCover> {
  const staged = await api.stageMedia(path, "cover");
  if (staged.type === "cover") return { coverId: staged.coverId, motionId: null };
  const media = staged.type === "ready" ? staged.media : await finish(staged, "cover");
  if (!media.posterId) throw new Error("Couldn't make a still frame of that file.");
  return { coverId: media.posterId, motionId: media.kind === "image" ? null : media.id };
}

/** Imports a file as a playlist or app background. */
export async function importBackground(path: string): Promise<Media> {
  const staged = await api.stageMedia(path, "background");
  if (staged.type === "ready") return staged.media;
  if (staged.type === "cover") throw new Error("That picture can't be used as a background.");
  return finish(staged, "background");
}

interface Probe {
  el: HTMLVideoElement | HTMLImageElement;
  width: number;
  height: number;
  durationMs: number | null;
  dispose: () => void;
}

async function finish(staged: Extract<StagedMedia, { type: "pending" }>, purpose: MediaPurpose): Promise<Media> {
  let probe: Probe | null = null;
  try {
    const url = mediaUrl(staged.id);
    probe = staged.kind === "video" ? await probeVideo(url) : await probeImage(url);
    if (staged.kind === "video") checkVideo(purpose, probe.width, probe.height, probe.durationMs ?? 0);
    const poster = firstFrame(probe.el, probe.width, probe.height);
    return await api.commitMedia(staged.id, purpose, probe.width, probe.height, probe.durationMs, poster);
  } catch (e) {
    await api.discardMedia(staged.id).catch(() => {});
    throw e instanceof Error ? e.message : e;
  } finally {
    probe?.dispose();
  }
}

function checkVideo(purpose: MediaPurpose, w: number, h: number, ms: number) {
  const l = VIDEO_LIMITS[purpose];
  const long = Math.max(w, h);
  const short = Math.min(w, h);
  const who = purpose === "cover" ? "Cover" : "Background";
  if (!w || !h) throw new Error("That video can't be read.");
  if (ms > l.ms + 50) throw new Error(`${who} videos can be up to ${l.ms / 1000} seconds long. This one is ${Math.round(ms / 1000)} s.`);
  if (purpose === "cover" && long > l.longPx) {
    throw new Error(`Cover videos can be up to ${l.longPx} px on the long side (720 px is plenty). This one is ${w}×${h}.`);
  }
  if (purpose === "background" && (long > l.longPx || short > l.shortPx)) {
    throw new Error(
      `Background videos can be up to 1920×1080. This one is ${w}×${h}; larger video costs too much to decode while music plays.`,
    );
  }
}

function once(target: EventTarget, event: string): Promise<void> {
  return new Promise((resolve) => target.addEventListener(event, () => resolve(), { once: true }));
}

function probeVideo(url: string): Promise<Probe> {
  return new Promise((resolve, reject) => {
    const v = document.createElement("video");
    v.muted = true;
    v.playsInline = true;
    v.preload = "auto";
    v.crossOrigin = "anonymous";
    const dispose = () => {
      v.removeAttribute("src");
      v.load();
    };
    const timer = window.setTimeout(() => reject(new Error("That video took too long to open.")), 20_000);
    v.addEventListener(
      "error",
      () => {
        window.clearTimeout(timer);
        reject(new Error("Undertone can't play this video. Save it as MP4 (H.264) or WebM (VP9) instead."));
      },
      { once: true },
    );
    v.addEventListener(
      "loadedmetadata",
      async () => {
        let duration = v.duration;
        // Recorded WebM often has no duration in its header; seeking far ahead makes the browser find it.
        if (!Number.isFinite(duration)) {
          const found = once(v, "durationchange");
          v.currentTime = 1e7;
          await found;
          duration = Number.isFinite(v.duration) ? v.duration : 0;
          const back = once(v, "seeked");
          v.currentTime = 0;
          await back;
        }
        if (v.readyState < 2) await once(v, "loadeddata");
        window.clearTimeout(timer);
        resolve({ el: v, width: v.videoWidth, height: v.videoHeight, durationMs: Math.round(duration * 1000), dispose });
      },
      { once: true },
    );
    v.src = url;
  });
}

async function probeImage(url: string): Promise<Probe> {
  const img = new Image();
  img.crossOrigin = "anonymous";
  img.src = url;
  try {
    await img.decode();
  } catch {
    throw new Error("That picture can't be read.");
  }
  return { el: img, width: img.naturalWidth, height: img.naturalHeight, durationMs: null, dispose: () => (img.src = "") };
}

/** The first frame as base64 JPEG, at most POSTER_PX on the long side. */
function firstFrame(el: CanvasImageSource, w: number, h: number): string {
  const scale = Math.min(1, POSTER_PX / Math.max(w, h, 1));
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(w * scale));
  canvas.height = Math.max(1, Math.round(h * scale));
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("Couldn't make a still frame of that file.");
  ctx.drawImage(el, 0, 0, canvas.width, canvas.height);
  return canvas.toDataURL("image/jpeg", 0.9).split(",")[1] ?? "";
}

/** Plain text for an import error (the backend sends strings, the probes throw Errors). */
export function importError(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
