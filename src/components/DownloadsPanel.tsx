import { Download, Loader2, Square, Trash2, X } from "lucide-react";
import { api } from "../lib/api";
import { usePlayer } from "../store/player";
import { usePlaylists } from "../store/playlists";
import { useSoulseek } from "../store/soulseek";
import { useUi } from "../store/ui";
import { JobStatus, mb } from "../views/SoulseekMissing";

const toastErr = (e: unknown) => usePlayer.getState().pushToast("error", String(e));

/** Active work: downloads not finished, plus songs waiting or being searched for. */
export function useDownloadsBadge(): number {
  const active = useSoulseek((s) => s.jobs.filter((j) => j.state === "queued" || j.state === "downloading").length);
  const searching = useSoulseek((s) => Object.values(s.finds).filter((f) => f.state === "searching" || f.state === "waiting").length);
  return active + searching;
}

/** Slide-over with Soulseek searches in progress and the download queue. */
export function DownloadsPanel() {
  const open = useUi((s) => s.downloadsOpen);
  const toggle = useUi((s) => s.toggleDownloads);
  const jobs = useSoulseek((s) => s.jobs);
  const finds = useSoulseek((s) => s.finds);
  const runs = useSoulseek((s) => s.findRuns);
  const playlists = usePlaylists((s) => s.playlists);
  if (!open) return null;

  const searching = Object.values(finds).filter((f) => f.state === "searching" || f.state === "waiting");
  const activeRuns = Object.entries(runs).filter(([, r]) => r.running);
  const ordered = [...jobs].sort((a, b) => {
    const rank = (s: string) => (s === "downloading" ? 0 : s === "queued" ? 1 : s === "failed" ? 2 : 3);
    return rank(a.state) - rank(b.state) || b.id - a.id;
  });
  const finished = jobs.filter((j) => j.state === "done" || j.state === "failed" || j.state === "cancelled").length;
  const totalSpeed = jobs.filter((j) => j.state === "downloading").reduce((s, j) => s + j.speed, 0);

  return (
    <aside
      className="absolute bottom-0 right-0 top-0 z-30 flex w-[420px] flex-col border-l border-line bg-elev/95 shadow-2xl backdrop-blur-xl animate-in"
      aria-label="Downloads"
    >
      <div className="flex items-center justify-between px-4 pb-2 pt-4">
        <div>
          <h2 className="text-[15px] font-semibold">Downloads</h2>
          <div className="text-[11.5px] text-faint">
            {jobs.length} file{jobs.length === 1 ? "" : "s"}
            {totalSpeed > 0 && ` · ${mb(totalSpeed)}/s`}
            {searching.length > 0 && ` · searching ${searching.length} song${searching.length === 1 ? "" : "s"}`}
          </div>
        </div>
        <div className="flex items-center gap-1">
          {finished > 0 && (
            <button
              onClick={() => api.slskClearFinished().catch(toastErr)}
              title="Remove finished, failed and cancelled downloads from the list"
              className="inline-flex items-center gap-1 rounded-full px-2.5 py-1 text-[11.5px] text-muted ring-1 ring-line hover:bg-hover hover:text-fg"
            >
              <Trash2 size={12} /> Clear finished
            </button>
          )}
          <button aria-label="Close downloads" onClick={toggle} className="ml-1 rounded-md p-1.5 text-muted hover:bg-hover hover:text-fg">
            <X size={16} />
          </button>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-4">
        {activeRuns.length > 0 && (
          <div className="mb-3">
            <div className="px-2 pb-1 pt-1 text-[11px] font-semibold uppercase tracking-wide text-faint">Searching Soulseek</div>
            {activeRuns.map(([pid, r]) => {
              const name = playlists.find((p) => p.id === Number(pid))?.name ?? "Playlist";
              return (
                <div key={pid} className="flex items-center gap-2 rounded-lg px-2 py-1.5 text-[12.5px] hover:bg-hover">
                  <Loader2 size={13} className="shrink-0 animate-spin text-accent" />
                  <button onClick={() => useUi.getState().setView({ kind: "review", id: Number(pid) })} className="min-w-0 flex-1 truncate text-left hover:underline">
                    {name}: {Math.min(r.done + 1, r.total)} of {r.total}
                  </button>
                  <button
                    onClick={() => api.stopFind(Number(pid)).catch(toastErr)}
                    className="inline-flex shrink-0 items-center gap-1 rounded-full px-2 py-0.5 text-[11px] text-muted ring-1 ring-line hover:text-fg"
                  >
                    <Square size={10} /> Stop
                  </button>
                </div>
              );
            })}
            {searching.slice(0, 8).map((f) => (
              <div key={f.refKey} className="flex items-center gap-2 rounded-lg px-2 py-1 pl-7 text-[12px] text-muted hover:bg-hover">
                <span className="min-w-0 flex-1 truncate">
                  {f.state === "searching" ? "Searching: " : "Waiting: "}
                  <span className="text-fg">{f.title}</span>
                  {f.artists.length > 0 && ` · ${f.artists.join(", ")}`}
                </span>
                <button aria-label={`Cancel search for ${f.title}`} title="Cancel this song's search" onClick={() => api.skipFind(f.refKey).catch(toastErr)} className="rounded p-0.5 text-faint hover:text-fg">
                  <X size={12} />
                </button>
              </div>
            ))}
            {searching.length > 8 && <div className="px-2 py-1 pl-7 text-[11.5px] text-faint">and {searching.length - 8} more…</div>}
          </div>
        )}

        <div className="px-2 pb-1 pt-1 text-[11px] font-semibold uppercase tracking-wide text-faint">Files</div>
        {ordered.length === 0 ? (
          <div className="flex flex-col items-center gap-2 px-4 py-10 text-center text-[12.5px] text-faint">
            <Download size={22} />
            Nothing downloading. Open an imported playlist and use "Get missing songs", or search on the Sources page.
          </div>
        ) : (
          ordered.map((j) => (
            <div key={j.id} className="rounded-lg px-2 py-2 hover:bg-hover" title={j.remotePath}>
              <div className="flex items-center gap-2">
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[12.5px]">{j.name}</div>
                  <div className="truncate text-[11px] text-faint">
                    {mb(j.size)} · {j.username}
                    {j.error && j.state === "failed" && <span className="text-warn"> · {j.error}</span>}
                  </div>
                </div>
                <JobStatus job={j} />
              </div>
            </div>
          ))
        )}
      </div>
    </aside>
  );
}
