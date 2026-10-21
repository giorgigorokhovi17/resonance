import { Music2 } from "lucide-react";
import { memo, useState, type CSSProperties } from "react";
import { useMotionSlot, type MotionPlace } from "../lib/motion";
import type { Media } from "../lib/types";
import { mediaUrl, useMotion } from "../store/media";

export function coverUrl(coverId: string | undefined | null, size: "thumb" | "full" = "thumb"): string | null {
  if (!coverId) return null;
  return `http://cover.localhost/${coverId}?s=${size}`;
}

/**
 * Artwork with a soft fade-in and a clean blank placeholder when there is none. With `motionId`
 * and `motion` (where it sits), a GIF or video plays over the still poster when allowed.
 */
export const Cover = memo(function Cover({
  coverId,
  motionId,
  motion,
  size = "thumb",
  className = "",
  rounded = "rounded-md",
  iconSize = 28,
}: {
  coverId?: string | null;
  motionId?: string | null;
  motion?: MotionPlace;
  size?: "thumb" | "full";
  className?: string;
  rounded?: string;
  iconSize?: number;
}) {
  const [failed, setFailed] = useState<string | null>(null);
  const [loaded, setLoaded] = useState<string | null>(null);
  const media = useMotion(motion ? motionId : null);
  const [ref, active] = useMotionSlot(motion ?? "tile", !!media);
  const url = coverUrl(coverId, size);
  if (!url || failed === url) {
    return (
      <div ref={ref} className={`flex items-center justify-center bg-hover text-faint/70 ${rounded} ${className}`}>
        <Music2 size={iconSize} strokeWidth={1.25} />
      </div>
    );
  }
  return (
    <div ref={ref} className={`relative overflow-hidden bg-hover ${rounded} ${className}`}>
      <img
        src={url}
        alt=""
        draggable={false}
        loading="lazy"
        decoding="async"
        crossOrigin="anonymous"
        onLoad={() => setLoaded(url)}
        onError={() => setFailed(url)}
        className={`h-full w-full object-cover transition-opacity duration-300 ${loaded === url ? "opacity-100" : "opacity-0"}`}
      />
      {active && media && <MotionMedia media={media} className="absolute inset-0 h-full w-full object-cover" />}
    </div>
  );
});

/** A GIF or a muted video loop, faded in over its poster once it has a frame. Never plays sound. */
export function MotionMedia({ media, className = "", style }: { media: Media; className?: string; style?: CSSProperties }) {
  const [ready, setReady] = useState(false);
  const cls = `${className} transition-opacity duration-300 ${ready ? "opacity-100" : "opacity-0"}`;
  if (media.kind === "video") {
    return (
      <video
        ref={(v) => {
          if (v) {
            v.muted = true;
            v.defaultMuted = true;
            v.volume = 0;
          }
        }}
        src={mediaUrl(media.id)}
        muted
        loop
        playsInline
        autoPlay
        preload="metadata"
        disablePictureInPicture
        disableRemotePlayback
        aria-hidden
        tabIndex={-1}
        data-motion
        // Lets backgrounds read frames to keep the scrim strong enough (Backdrop.tsx).
        crossOrigin="anonymous"
        onPlaying={() => setReady(true)}
        className={cls}
        style={style}
      />
    );
  }
  return (
    <img
      src={mediaUrl(media.id)}
      alt=""
      aria-hidden
      data-motion
      crossOrigin="anonymous"
      draggable={false}
      decoding="async"
      onLoad={() => setReady(true)}
      className={cls}
      style={style}
    />
  );
}
