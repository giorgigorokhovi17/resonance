import { ListMusic } from "lucide-react";
import { memo } from "react";
import type { Playlist } from "../lib/types";
import { Cover } from "./Cover";

/** Playlist artwork: the custom photo, else a 2x2 collage of its songs, else a gradient tile. */
export const PlaylistCover = memo(function PlaylistCover({
  playlist,
  className = "",
  rounded = "rounded-lg",
  iconSize = 40,
  size = "thumb",
}: {
  playlist: Pick<Playlist, "coverId" | "previewCovers">;
  className?: string;
  rounded?: string;
  iconSize?: number;
  size?: "thumb" | "full";
}) {
  if (playlist.coverId) {
    return <Cover coverId={playlist.coverId} size={size} className={className} rounded={rounded} iconSize={iconSize} />;
  }
  const covers = playlist.previewCovers ?? [];
  if (covers.length >= 4) {
    return (
      <div className={`grid grid-cols-2 grid-rows-2 overflow-hidden ${rounded} ${className}`}>
        {covers.slice(0, 4).map((c) => (
          <Cover key={c} coverId={c} className="h-full w-full" rounded="" iconSize={iconSize / 2} />
        ))}
      </div>
    );
  }
  if (covers.length > 0) {
    return <Cover coverId={covers[0]} size={size} className={className} rounded={rounded} iconSize={iconSize} />;
  }
  return (
    <div className={`flex items-center justify-center bg-gradient-to-br from-[#3a3f5c] to-[#1c1f2b] text-white/80 ${rounded} ${className}`}>
      <ListMusic size={iconSize} strokeWidth={1.4} />
    </div>
  );
});
