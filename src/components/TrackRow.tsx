import { AudioLines, Check, EllipsisVertical, X } from "lucide-react";
import { useEffect, type ReactNode } from "react";
import { displayArtist, isHiRes } from "../lib/format";
import type { Track } from "../lib/types";
import { liveCoverId } from "../store/library";
import { useListStyle } from "../store/listStyle";
import { usePlayer } from "../store/player";
import { useUi } from "../store/ui";
import { Cover } from "./Cover";

/** A file's format and resolution, as a small tag. */
export function Quality({ t }: { t: Track }) {
  const hi = isHiRes(t);
  const label = t.bitDepth && t.sampleRate ? `${t.bitDepth}/${+(t.sampleRate / 1000).toFixed(1)}` : t.bitrate && t.bitrate >= 32 ? `${t.bitrate}k` : "";
  return (
    <span className="flex shrink-0 items-center gap-1.5 truncate text-[11.5px] mobile:overflow-visible">
      <span className={`rounded px-1 py-px text-[10px] font-semibold ring-1 ${hi ? "text-accent ring-accent/40" : "text-faint ring-line"}`}>
        {t.codec || "?"}
      </span>
      <span className="text-muted">{label}</span>
    </span>
  );
}

const COVER = { compact: "h-9 w-9", normal: "h-12 w-12", large: "h-[60px] w-[60px]" };

/**
 * A song on a phone: tap plays it; the dots or a long press open its menu. With `selected` set
 * (true or false) the list is being selected from, and a tap ticks the song instead.
 */
export function TrackRow({
  track,
  subtitle,
  dimmed,
  selected,
  onPlay,
  onMenu,
  trailing,
}: {
  track: Track;
  subtitle?: ReactNode;
  dimmed?: boolean;
  selected?: boolean;
  onPlay?: () => void;
  onMenu: (e: React.MouseEvent) => void;
  trailing?: ReactNode;
}) {
  const current = usePlayer((s) => s.status?.track?.path === track.path);
  const playing = usePlayer((s) => s.status?.state === "playing");
  const style = useListStyle();
  const selecting = selected !== undefined;
  return (
    <div
      onClick={onPlay}
      onContextMenu={selecting ? (e) => e.preventDefault() : onMenu}
      className={`flex h-full items-center gap-3 pl-4 pr-1 transition-colors active:bg-hover ${selected ? "bg-accent/10" : ""} ${dimmed ? "opacity-50" : ""}`}
    >
      {selecting && (
        <span className={`flex h-[22px] w-[22px] shrink-0 items-center justify-center rounded-full ring-2 ${selected ? "bg-accent text-on-accent ring-accent" : "ring-faint"}`}>
          {selected && <Check size={14} strokeWidth={3} />}
        </span>
      )}
      {style.cover && (
        <div className="relative shrink-0">
          <Cover coverId={liveCoverId(track)} className={COVER[style.size]} rounded="rounded-md" iconSize={18} />
          {current && (
            <span className="absolute inset-0 flex items-center justify-center rounded-md bg-black/50">
              <AudioLines size={18} className={`text-white ${playing ? "animate-pulse" : ""}`} />
            </span>
          )}
        </div>
      )}
      <div className="min-w-0 flex-1">
        <div className={`truncate text-[15px] ${style.bold ? "font-bold" : ""} ${current ? "font-semibold text-accent" : ""}`}>{track.title}</div>
        {style.artist && <div className="truncate text-[13px] text-muted">{subtitle ?? displayArtist(track)}</div>}
      </div>
      {style.quality && <Quality t={track} />}
      {trailing}
      {!selecting && (
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
      )}
    </div>
  );
}

/** Across the top of a list while songs are being selected. */
export function SelectionBar({
  count,
  total,
  onAll,
  onClose,
  onMenu,
}: {
  count: number;
  total: number;
  onAll: (all: boolean) => void;
  onClose: () => void;
  onMenu: (e: React.MouseEvent) => void;
}) {
  const all = count === total;
  return (
    <div className="glass-bar absolute inset-x-0 top-0 z-20 flex h-14 items-center gap-1 px-1 animate-in">
      <button aria-label="Stop selecting" onClick={onClose} className="flex h-11 w-11 items-center justify-center">
        <X size={21} />
      </button>
      <span className="min-w-0 flex-1 truncate text-[15px] font-semibold">{count} selected</span>
      <button onClick={() => onAll(!all)} className="h-11 rounded-full px-3 text-[14px] font-medium text-accent">
        {all ? "Select none" : "Select all"}
      </button>
      <button aria-label="Options for the selected songs" disabled={!count} onClick={onMenu} className="flex h-11 w-10 items-center justify-center disabled:opacity-30">
        <EllipsisVertical size={20} />
      </button>
    </div>
  );
}

/** While `fn` is set, the phone's Back button runs it (see useBackButton). */
export function useBackAction(fn: (() => void) | null) {
  const on = !!fn;
  useEffect(() => {
    if (!on) return;
    useUi.setState({ backAction: fn });
    return () => useUi.setState({ backAction: null });
  }, [on]);
}
