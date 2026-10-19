import { AudioLines, GripVertical, Trash2, X } from "lucide-react";
import { useMemo, type ReactNode } from "react";
import { api } from "../lib/api";
import { fmtTime, displayArtist } from "../lib/format";
import type { QueueItem } from "../lib/types";
import { usePlayer } from "../store/player";
import { liveCoverId, useLibrary } from "../store/library";
import { openMenu, SEP } from "./ContextMenu";
import { Cover } from "./Cover";
import { ReorderList } from "./ReorderList";
import { trackMenuItems } from "./trackMenu";

export function QueueList({ compact = false, header }: { compact?: boolean; header?: ReactNode }) {
  const queue = usePlayer((s) => s.queue);
  const playing = usePlayer((s) => s.status?.state === "playing");
  useLibrary((s) => s.byId); // re-render when artwork visibility changes
  const currentIdx = useMemo(
    () => queue.items.findIndex((i) => i.uid === queue.currentUid),
    [queue.items, queue.currentUid],
  );

  const onMove = async (from: number, to: number) => {
    // Optimistic reorder; the backend echoes the authoritative queue.
    const items = [...queue.items];
    const [it] = items.splice(from, 1);
    items.splice(to, 0, it);
    usePlayer.getState().setQueue({ ...queue, items });
    await api.moveQueueItem(from, to);
  };

  return (
    <ReorderList<QueueItem>
      items={queue.items}
      getKey={(i) => i.uid}
      rowHeight={compact ? 48 : 52}
      onMove={onMove}
      header={header}
      scrollToIndex={currentIdx >= 0 ? currentIdx : undefined}
      renderRow={(item, index) => {
        const isCur = item.uid === queue.currentUid;
        const isPast = currentIdx >= 0 && index < currentIdx;
        const t = item.track;
        return (
          <div
            className={`group mx-2 flex h-full items-center gap-3 rounded-md px-2 transition-colors duration-150 ${isCur ? "bg-accent/12" : "hover:bg-hover"} ${
              isPast ? "opacity-55" : ""
            }`}
            onDoubleClick={() => api.playQueueItem(item.uid)}
            onContextMenu={(e) =>
              openMenu(e, [
                { label: "Play", onClick: () => api.playQueueItem(item.uid) },
                ...trackMenuItems([t]).slice(1),
                SEP,
                {
                  label: "Remove from queue",
                  icon: <Trash2 size={14} />,
                  danger: true,
                  disabled: isCur,
                  onClick: () => api.removeFromQueue([item.uid]),
                },
              ])
            }
          >
            <GripVertical size={14} className="shrink-0 cursor-grab text-faint opacity-0 group-hover:opacity-100" />
            {!compact && <span className="tabular w-6 shrink-0 text-right text-[12px] text-faint">{index + 1}</span>}
            <div className="relative shrink-0">
              <Cover coverId={liveCoverId(t)} className={compact ? "h-9 w-9" : "h-10 w-10"} rounded="rounded" iconSize={16} />
              {isCur && (
                <span className="absolute inset-0 flex items-center justify-center rounded bg-black/50">
                  <AudioLines size={16} className={`text-white ${playing ? "animate-pulse" : ""}`} />
                </span>
              )}
            </div>
            <div className="min-w-0 flex-1">
              <div className={`truncate text-[13px] ${isCur ? "font-semibold text-accent" : ""}`}>{t.title}</div>
              <div className="truncate text-[12px] text-muted">
                {displayArtist(t)}
                {!compact && t.album ? <span className="text-faint"> · {t.album}</span> : null}
              </div>
            </div>
            <span className="tabular shrink-0 text-[12px] text-muted">{fmtTime(t.duration)}</span>
            <button
              aria-label="Remove from queue"
              title={isCur ? "Currently playing" : "Remove"}
              disabled={isCur}
              onClick={() => api.removeFromQueue([item.uid])}
              className="shrink-0 rounded p-1 text-faint opacity-0 hover:bg-hover hover:text-fg group-hover:opacity-100 disabled:hidden"
            >
              <X size={14} />
            </button>
          </div>
        );
      }}
    />
  );
}
