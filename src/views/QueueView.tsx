import { ListOrdered, ListPlus, Trash2, X } from "lucide-react";
import { promptText } from "../components/Dialog";
import { EmptyState, SecondaryButton } from "../components/EmptyState";
import { QueueList } from "../components/QueueList";
import { ViewHeader } from "../components/ViewHeader";
import { api } from "../lib/api";
import { fmtDuration } from "../lib/format";
import { usePlayer } from "../store/player";
import { useUi } from "../store/ui";

async function saveQueue() {
  const name = await promptText("Save queue as playlist", "Queue " + new Date().toLocaleDateString(), "Save");
  if (!name) return;
  try {
    await api.saveQueueAsPlaylist(name);
    usePlayer.getState().pushToast("info", `Saved the queue as "${name}".`);
  } catch (e) {
    usePlayer.getState().pushToast("error", String(e));
  }
}

function QueueActions({ compact }: { compact?: boolean }) {
  const has = usePlayer((s) => s.queue.items.length > 1);
  return (
    <>
      <SecondaryButton onClick={saveQueue} title="Save as playlist">
        <ListPlus size={15} /> {!compact && "Save as playlist"}
      </SecondaryButton>
      {has && (
        <SecondaryButton onClick={() => api.clearQueue()} title="Clear everything except the current track">
          <Trash2 size={15} /> {!compact && "Clear"}
        </SecondaryButton>
      )}
    </>
  );
}

export function QueueView() {
  const items = usePlayer((s) => s.queue.items);
  if (!items.length) {
    return (
      <EmptyState
        icon={<ListOrdered size={28} strokeWidth={1.4} />}
        title="The queue is empty"
        body="Play an album, playlist or track, or use “Add to queue” from any right-click menu."
      />
    );
  }
  const total = items.reduce((s, i) => s + i.track.duration, 0);
  return (
    <QueueList
      header={
        <ViewHeader
          title="Queue"
          subtitle={`${items.length.toLocaleString()} tracks · ${fmtDuration(total)} · drag to reorder`}
          actions={<QueueActions />}
        />
      }
    />
  );
}

/** Slide-over queue panel (toggled with Q). */
export function QueuePanel() {
  const open = useUi((s) => s.queueOpen);
  const toggle = useUi((s) => s.toggleQueue);
  const count = usePlayer((s) => s.queue.items.length);
  if (!open) return null;
  return (
    <aside
      className="absolute bottom-0 right-0 top-0 z-30 flex w-[380px] flex-col border-l border-line bg-elev/95 shadow-2xl backdrop-blur-xl animate-in"
      aria-label="Queue"
    >
      <div className="flex items-center justify-between px-4 pb-2 pt-4">
        <div>
          <h2 className="text-[15px] font-semibold">Queue</h2>
          <div className="text-[11.5px] text-faint">{count} tracks</div>
        </div>
        <div className="flex items-center gap-1">
          <QueueActions compact />
          <button aria-label="Close queue" onClick={toggle} className="ml-1 rounded-md p-1.5 text-muted hover:bg-hover hover:text-fg">
            <X size={16} />
          </button>
        </div>
      </div>
      <div className="min-h-0 flex-1">
        {count ? <QueueList compact /> : <div className="px-4 py-8 text-center text-[12.5px] text-faint">Nothing queued.</div>}
      </div>
    </aside>
  );
}
