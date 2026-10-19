import { Music2 } from "lucide-react";
import { memo, useState } from "react";

export function coverUrl(coverId: string | undefined | null, size: "thumb" | "full" = "thumb"): string | null {
  if (!coverId) return null;
  return `http://cover.localhost/${coverId}?s=${size}`;
}

/** Artwork with a soft fade-in and a clean blank placeholder when there is none. */
export const Cover = memo(function Cover({
  coverId,
  size = "thumb",
  className = "",
  rounded = "rounded-md",
  iconSize = 28,
}: {
  coverId?: string | null;
  size?: "thumb" | "full";
  className?: string;
  rounded?: string;
  iconSize?: number;
}) {
  const [failed, setFailed] = useState<string | null>(null);
  const [loaded, setLoaded] = useState<string | null>(null);
  const url = coverUrl(coverId, size);
  if (!url || failed === url) {
    return (
      <div className={`flex items-center justify-center bg-hover text-faint/70 ${rounded} ${className}`}>
        <Music2 size={iconSize} strokeWidth={1.25} />
      </div>
    );
  }
  return (
    <div className={`overflow-hidden bg-hover ${rounded} ${className}`}>
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
    </div>
  );
});
