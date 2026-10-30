import { AudioLines, EllipsisVertical } from "lucide-react";
import type { ReactNode } from "react";
import { displayArtist } from "../lib/format";
import type { Track } from "../lib/types";
import { liveCoverId } from "../store/library";
import { usePlayer } from "../store/player";
import { Cover } from "./Cover";

/** Height of a phone song row. */
export const TRACK_ROW = 64;

/** A song on a phone: tap plays it; the dots or a long press open its menu. */
export function TrackRow({
  track,
  subtitle,
  dimmed,
  onPlay,
  onMenu,
  leading,
  trailing,
}: {
  track: Track;
  subtitle?: ReactNode;
  dimmed?: boolean;
  onPlay?: () => void;
  onMenu: (e: React.MouseEvent) => void;
  leading?: ReactNode;
  trailing?: ReactNode;
}) {
  const current = usePlayer((s) => s.status?.track?.path === track.path);
  const playing = usePlayer((s) => s.status?.state === "playing");
  return (
    <div
      onClick={onPlay}
      onContextMenu={onMenu}
      className={`flex h-full items-center gap-3 pl-4 pr-1 active:bg-hover ${dimmed ? "opacity-50" : ""}`}
    >
      {leading}
      <div className="relative shrink-0">
        <Cover coverId={liveCoverId(track)} className="h-12 w-12" rounded="rounded-md" iconSize={18} />
        {current && (
          <span className="absolute inset-0 flex items-center justify-center rounded-md bg-black/50">
            <AudioLines size={18} className={`text-white ${playing ? "animate-pulse" : ""}`} />
          </span>
        )}
      </div>
      <div className="min-w-0 flex-1">
        <div className={`truncate text-[15px] ${current ? "font-medium text-accent" : ""}`}>{track.title}</div>
        <div className="truncate text-[13px] text-muted">{subtitle ?? displayArtist(track)}</div>
      </div>
      {trailing}
      <button
        aria-label={`Options for ${track.title}`}
        onClick={(e) => {
          e.stopPropagation();
          onMenu(e);
        }}
        className="flex h-11 w-10 shrink-0 items-center justify-center text-muted active:text-fg"
      >
        <EllipsisVertical size={19} />
      </button>
    </div>
  );
}
