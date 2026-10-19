import { getCurrentWindow } from "@tauri-apps/api/window";
import { Maximize2, Pause, Play, SkipBack, SkipForward, X } from "lucide-react";
import { Cover } from "../components/Cover";
import { FormatBadge } from "../components/FormatBadge";
import { api } from "../lib/api";
import { displayArtist } from "../lib/format";
import { livePosition, usePlayer } from "../store/player";
import { useLiveCover } from "../store/library";
import { useEffect, useState } from "react";

/** Compact always-on-top player (Ctrl+M toggles). */
export function MiniPlayer({ onExit }: { onExit: () => void }) {
  const status = usePlayer((s) => s.status);
  const queue = usePlayer((s) => s.queue);
  const track = status?.track ?? queue.items.find((i) => i.uid === queue.currentUid)?.track ?? null;
  const playing = status?.state === "playing";
  const coverId = useLiveCover(track);
  const [frac, setFrac] = useState(0);

  useEffect(() => {
    let raf = 0;
    const tick = () => {
      const d = usePlayer.getState().status?.duration ?? 0;
      setFrac(d > 0 ? Math.min(1, livePosition() / d) : 0);
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, []);

  const btn = "flex h-8 w-8 items-center justify-center rounded-full text-fg/80 hover:bg-hover hover:text-fg";

  return (
    <div data-tauri-drag-region className="relative flex h-full select-none items-center gap-3 overflow-hidden bg-elev px-3">
      <Cover coverId={coverId} className="h-[84px] w-[84px] shrink-0 shadow-lg" rounded="rounded-lg" iconSize={28} />
      <div data-tauri-drag-region className="min-w-0 flex-1">
        <div className="truncate text-[13.5px] font-semibold">{track?.title ?? "Nothing playing"}</div>
        <div className="truncate text-[12px] text-muted">{track ? displayArtist(track) : ""}</div>
        <div className="mt-0.5 h-4 overflow-hidden">
          <FormatBadge />
        </div>
        <div className="mt-1 flex items-center gap-1">
          <button aria-label="Previous" className={btn} onClick={() => api.previous()}>
            <SkipBack size={15} fill="currentColor" />
          </button>
          <button
            aria-label={playing ? "Pause" : "Play"}
            className="flex h-9 w-9 items-center justify-center rounded-full bg-fg text-bg hover:scale-105"
            onClick={() => api.togglePlay()}
          >
            {playing ? <Pause size={16} fill="currentColor" strokeWidth={0} /> : <Play size={16} fill="currentColor" strokeWidth={0} className="translate-x-px" />}
          </button>
          <button aria-label="Next" className={btn} onClick={() => api.next()}>
            <SkipForward size={15} fill="currentColor" />
          </button>
        </div>
      </div>
      <div className="flex flex-col gap-1 self-start pt-2">
        <button aria-label="Exit mini player" title="Full player (Ctrl+M)" className={btn} onClick={onExit}>
          <Maximize2 size={14} />
        </button>
        <button aria-label="Close" title="Close" className={btn} onClick={() => getCurrentWindow().close()}>
          <X size={15} />
        </button>
      </div>
      <div className="absolute bottom-0 left-0 right-0 h-[3px] bg-fg/10">
        <div className="accent-gradient h-full" style={{ width: `${frac * 100}%` }} />
      </div>
    </div>
  );
}
