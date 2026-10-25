import { listen } from "@tauri-apps/api/event";
import { RotateCcw, X } from "lucide-react";
import { useEffect, useState } from "react";
import { Cover } from "../../components/Cover";
import { api } from "../../lib/api";
import type { DeletedTrack } from "../../lib/types";
import { usePlayer } from "../../store/player";
import { Section } from "../SettingsView";

function deletedOn(secs: number): string {
  return new Date(secs * 1000).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });
}

/** Songs deleted from disk with "Delete from disk…", to put back from the Recycle Bin. */
export function RecentlyDeletedSection() {
  const [rows, setRows] = useState<DeletedTrack[] | null>(null);
  const [busy, setBusy] = useState<number | null>(null);
  const toast = usePlayer((s) => s.pushToast);

  useEffect(() => {
    const load = () => api.getDeletedTracks().then(setRows, (e) => toast("error", String(e)));
    load();
    const off = listen("deleted:changed", load);
    return () => {
      off.then((f) => f());
    };
  }, [toast]);

  const restore = async (d: DeletedTrack) => {
    setBusy(d.id);
    try {
      await api.restoreDeletedTracks([d.id]);
      toast("info", `Restored "${d.title}".`);
    } catch (e) {
      toast("error", `Couldn't restore "${d.title}". ${e}`);
      setRows(await api.getDeletedTracks());
    } finally {
      setBusy(null);
    }
  };

  const forget = async (ids: number[]) => {
    try {
      await api.forgetDeletedTracks(ids);
    } catch (e) {
      toast("error", String(e));
    }
  };

  const gone = rows?.filter((d) => !d.inBin) ?? [];

  return (
    <Section
      title="Recently deleted"
      description="Songs you deleted from disk are in the Windows Recycle Bin. Restore puts the file back in its folder, and its playlists, artwork and lyrics come back with it."
    >
      <div className="overflow-hidden rounded-lg ring-1 ring-line">
        {rows && rows.length === 0 && <div className="px-4 py-4 text-[12.5px] text-faint">Nothing deleted.</div>}
        {rows?.map((d) => (
          <div key={d.id} className="flex items-center gap-3 border-b border-line px-4 py-2.5 last:border-b-0">
            <Cover coverId={d.coverId} className="h-9 w-9 shrink-0" iconSize={16} />
            <div className="min-w-0 flex-1">
              <div className="truncate text-[13px]">
                {d.title}
                {d.artist && <span className="text-muted"> · {d.artist}</span>}
              </div>
              <div className="truncate text-[11.5px] text-faint" title={d.path}>
                {d.inBin ? `Deleted ${deletedOn(d.deletedAt)}` : "No longer in the Recycle Bin"} · {d.path}
              </div>
            </div>
            {d.inBin && (
              <button
                onClick={() => restore(d)}
                disabled={busy !== null}
                className="flex items-center gap-1.5 rounded-full px-3 py-1 text-[12px] font-medium ring-1 ring-line hover:bg-hover disabled:opacity-50"
              >
                <RotateCcw size={13} className={busy === d.id ? "animate-spin" : ""} /> Restore
              </button>
            )}
            <button
              aria-label={`Remove ${d.title} from the list`}
              title="Remove from list (stays in the Recycle Bin)"
              onClick={() => forget([d.id])}
              className="rounded-md p-1.5 text-faint hover:bg-hover hover:text-fg"
            >
              <X size={15} />
            </button>
          </div>
        ))}
      </div>
      {gone.length > 1 && (
        <button
          onClick={() => forget(gone.map((d) => d.id))}
          className="mt-3 rounded-full px-4 py-1.5 text-[12.5px] font-medium text-muted ring-1 ring-line hover:bg-hover"
        >
          Clear {gone.length} no longer in the Recycle Bin
        </button>
      )}
    </Section>
  );
}
