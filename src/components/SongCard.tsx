import { AudioLines, Check, Pause, Play } from "lucide-react";
import { memo } from "react";
import { api } from "../lib/api";
import { displayArtist, isHiRes } from "../lib/format";
import type { Track } from "../lib/types";
import { openMenu } from "./ContextMenu";
import { Cover } from "./Cover";
import { trackMenuItems } from "./trackMenu";

/**
 * Large artwork card for a single song. With `onSelect`, Ctrl/Shift-click (or any click while
 * `selecting`) and the corner checkbox select instead of play.
 */
export const SongCard = memo(function SongCard({
  track,
  isCurrent,
  playing,
  onPlay,
  selected = false,
  selecting = false,
  onSelect,
  onContextMenu,
}: {
  track: Track;
  isCurrent: boolean;
  playing: boolean;
  onPlay: () => void;
  selected?: boolean;
  selecting?: boolean;
  onSelect?: (e: React.MouseEvent) => void;
  onContextMenu?: (e: React.MouseEvent) => void;
}) {
  const click = (e: React.MouseEvent) => {
    if (onSelect && (selecting || e.ctrlKey || e.shiftKey)) return onSelect(e);
    isCurrent ? api.togglePlay() : onPlay();
  };
  return (
    <div
      data-motion-hover
      className={`group min-w-0 cursor-pointer select-none rounded-lg p-2.5 transition-colors duration-200 ${
        selected ? "bg-accent/15 ring-1 ring-accent/50" : "hover:bg-hover"
      }`}
      onClick={click}
      onContextMenu={(e) => (onContextMenu ? onContextMenu(e) : openMenu(e, trackMenuItems([track], { playAll: onPlay })))}
      title={`${track.title} — ${displayArtist(track)}`}
    >
      <div className="relative aspect-square w-full">
        <Cover
          coverId={track.coverId}
          motionId={track.motionId}
          motion="tile"
          className="h-full w-full shadow-[0_8px_24px_-8px_rgba(0,0,0,0.5)]"
          iconSize={40}
        />
        {isHiRes(track) && (
          <span className="absolute left-2 top-2 rounded bg-black/55 px-1.5 py-px text-[9.5px] font-bold tracking-wider text-[#f2c27b] backdrop-blur">
            HI-RES
          </span>
        )}
        {onSelect && (
          <button
            aria-label={selected ? "Deselect" : "Select"}
            aria-pressed={selected}
            onClick={(e) => {
              e.stopPropagation();
              onSelect(e);
            }}
            className={`absolute right-2 top-2 flex h-6 w-6 items-center justify-center rounded-full ring-2 backdrop-blur transition-opacity duration-150 ${
              selected
                ? "bg-accent text-on-accent opacity-100 ring-accent"
                : `bg-black/45 text-transparent ring-white/80 hover:bg-black/60 ${selecting ? "opacity-100" : "opacity-0 group-hover:opacity-100"}`
            }`}
          >
            <Check size={14} strokeWidth={3} />
          </button>
        )}
        <span
          className={`absolute bottom-2 right-2 flex h-11 w-11 items-center justify-center rounded-full bg-accent text-on-accent shadow-lg transition-all duration-200 hover:scale-105 ${
            isCurrent ? "translate-y-0 opacity-100" : "translate-y-1.5 opacity-0 group-hover:translate-y-0 group-hover:opacity-100"
          }`}
        >
          {isCurrent && playing ? (
            <Pause size={18} fill="currentColor" strokeWidth={0} />
          ) : (
            <Play size={18} fill="currentColor" strokeWidth={0} className="translate-x-px" />
          )}
        </span>
      </div>
      <div className={`mt-2.5 flex items-center gap-1.5 truncate text-[13.5px] font-medium ${isCurrent ? "text-accent" : ""}`}>
        {isCurrent && <AudioLines size={14} className={`shrink-0 ${playing ? "animate-pulse" : ""}`} />}
        <span className="truncate">{track.title}</span>
      </div>
      <div className="truncate text-[12.5px] text-muted">{displayArtist(track)}</div>
    </div>
  );
});
