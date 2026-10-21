import { useEffect, useMemo } from "react";
import { create } from "zustand";
import { toneLevel, toneOf, useBackdropTone } from "../hooks/useCoverColor";
import { useIsDark } from "../hooks/useAppearance";
import { useMotionSlot, type MotionPlace } from "../lib/motion";
import { DEFAULT_APPEARANCE, DIM_DEFAULT, readableDim, resolveTheme, type BackdropReadability } from "../lib/theme";
import type { Backdrop as BackdropOptions, Media } from "../lib/types";
import { mediaUrl, useMedia } from "../store/media";
import { usePlayer } from "../store/player";
import { coverUrl, MotionMedia } from "./Cover";

export type Scrim = "bg" | "elev";

/**
 * Riskiest tones seen in a playing GIF or video, by `mediaId:mode`. Later frames can be brighter
 * (or darker) than the first one the poster comes from, e.g. a video that fades in from black.
 */
const useSeenTones = create<{ tones: Record<string, string> }>(() => ({ tones: {} }));
const toneKey = (mediaId: string, dark: boolean) => `${mediaId}:${dark ? "d" : "l"}`;
const riskier = (a: string | null, b: string | null | undefined, dark: boolean) =>
  !a || !b ? a ?? b ?? null : (toneLevel(b) > toneLevel(a)) === dark ? b : a;

/**
 * How strong a backdrop's scrim is in the current mode, raised when needed so text over it
 * passes WCAG AA. `scrim` is the surface colour laid over the picture.
 */
export function useBackdropReadability(
  b: BackdropOptions | null | undefined,
  scrim: Scrim,
): BackdropReadability & { ready: boolean; chosen: number } {
  const dark = useIsDark();
  const a = usePlayer((s) => s.settings?.appearance) ?? DEFAULT_APPEARANCE;
  const media = useMedia((s) => (b ? s.byId.get(b.mediaId) : undefined));
  const poster = useBackdropTone(media?.posterId, dark);
  const seen = useSeenTones((s) => (b ? s.tones[toneKey(b.mediaId, dark)] : undefined));
  const tone = poster && riskier(poster, seen, dark);
  return useMemo(() => {
    const v = resolveTheme(a, dark, null).vars;
    const chosen = b?.dim ?? (dark ? DIM_DEFAULT.dark : DIM_DEFAULT.light);
    const r = readableDim(tone, v[scrim === "bg" ? "--c-bg" : "--c-elev"], [v["--c-fg"], v["--c-muted"]], chosen);
    return { ...r, ready: !!tone, chosen };
  }, [a, dark, b?.dim, tone, scrim]);
}

/**
 * While a GIF or video plays its first loop, checks a frame every 1.5 s and records it if it's
 * riskier for text than anything seen so far. Stops after one loop; the result is kept.
 */
function useFrameTones(container: React.RefObject<HTMLElement | null>, media: Media | undefined, active: boolean, dark: boolean) {
  useEffect(() => {
    if (!active || !media) return;
    const key = toneKey(media.id, dark);
    let samples = 0;
    const loop = Math.max(3000, media.durationMs ?? 6000);
    const id = window.setInterval(() => {
      const el = container.current?.querySelector<HTMLVideoElement | HTMLImageElement>("[data-motion]");
      if (!el) return;
      const ready = el instanceof HTMLVideoElement ? el.readyState >= 2 : el.complete;
      if (!ready) return;
      try {
        const w = el instanceof HTMLVideoElement ? el.videoWidth : el.naturalWidth;
        const h = el instanceof HTMLVideoElement ? el.videoHeight : el.naturalHeight;
        const t = toneOf(el, w, h, dark);
        const cur = useSeenTones.getState().tones[key];
        const next = riskier(cur ?? null, t, dark);
        if (next && next !== cur) useSeenTones.setState((s) => ({ tones: { ...s.tones, [key]: next } }));
      } catch {
        // A frame that can't be read leaves the poster's tone in charge.
      }
      if (++samples * 1500 > loop + 1500) window.clearInterval(id);
    }, 1500);
    return () => window.clearInterval(id);
  }, [container, media, active, dark]);
}

/**
 * A custom photo, GIF or video filling its container, with blur and a scrim so text stays
 * readable. A GIF or video shows its poster until it may move (see lib/motion.ts).
 */
export function Backdrop({
  backdrop,
  place,
  scrim = "bg",
  className = "absolute inset-0",
}: {
  backdrop: BackdropOptions;
  place: MotionPlace;
  scrim?: Scrim;
  className?: string;
}) {
  const media = useMedia((s) => s.byId.get(backdrop.mediaId));
  const read = useBackdropReadability(backdrop, scrim);
  const dark = useIsDark();
  const [ref, active] = useMotionSlot(place, !!media && media.kind !== "image");
  useFrameTones(ref, media, active, dark);
  if (!media) return null;
  const fit = backdrop.fit === "contain" ? "object-contain" : "object-cover";
  const still = media.kind === "image" ? mediaUrl(media.id) : coverUrl(media.posterId, "full");
  return (
    <div
      ref={ref}
      aria-hidden
      className={`pointer-events-none overflow-hidden transition-opacity duration-500 ${read.ready ? "opacity-100" : "opacity-0"} ${className}`}
    >
      <div
        className="absolute inset-0"
        // Scaled up a little so blurred edges don't show the surface underneath.
        style={backdrop.blur > 0 ? { filter: `blur(${backdrop.blur}px)`, transform: "scale(1.06)" } : undefined}
      >
        {still && <img src={still} alt="" draggable={false} decoding="async" className={`absolute inset-0 h-full w-full ${fit}`} />}
        {active && <MotionMedia media={media} className={`absolute inset-0 h-full w-full ${fit}`} />}
      </div>
      <div
        className="absolute inset-0 transition-opacity duration-1000"
        style={{ background: `var(--c-${scrim})`, opacity: read.dim }}
      />
    </div>
  );
}
