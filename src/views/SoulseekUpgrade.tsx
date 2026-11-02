import { Download, Loader2, RotateCw, Square, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { api } from "../lib/api";
import { flyToDownloads } from "../lib/fly";
import type { FoundFile, PlaylistEntry } from "../lib/types";
import { usePlayer } from "../store/player";
import { useSoulseek } from "../store/soulseek";
import { useUi } from "../store/ui";
import { FormatBadge, JobStatus, mb } from "./SoulseekMissing";

const toastErr = (e: unknown) => usePlayer.getState().pushToast("error", String(e));

/** Stands for the file to replace in searches and downloads (`UPGRADE_KEY` in missing.rs). */
const keyOf = (path: string) => `upgrade:${path}`;

/**
 * Better copies of a playlist's YouTube downloads, on Soulseek. Searching starts on opening and
 * keeps going if this is closed; nothing downloads until you press Download. A finished download
 * takes the YouTube file's place in every playlist, and its row leaves this list.
 */
export function SoulseekUpgrade({ playlistId, entries, onClose }: { playlistId: number; entries: PlaylistEntry[]; onClose: () => void }) {
  const status = useSoulseek((s) => s.status);
  const jobs = useSoulseek((s) => s.jobs);
  const finds = useSoulseek((s) => s.finds);
  const run = useSoulseek((s) => s.findRuns[playlistId]);
  const setView = useUi((s) => s.setView);
  /** Chosen file per song: a path, "" for none. Unset = the recommended one. */
  const [pick, setPick] = useState<Record<string, string>>({});

  // A song can be in a playlist twice.
  const songs = useMemo(() => [...new Map(entries.map((e) => [e.track.path, e.track])).values()], [entries]);
  const signedIn = status.state === "signedIn";
  const running = !!run?.running;
  const searched = songs.some((t) => finds[keyOf(t.path)]);

  const scan = async () => {
    try {
      const total = await api.findBetterCopies(playlistId);
      useSoulseek.setState((s) => ({ findRuns: { ...s.findRuns, [playlistId]: { done: 0, total, running: true } } }));
    } catch (e) {
      toastErr(e);
    }
  };
  useEffect(() => {
    if (signedIn && !running && !searched) scan();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signedIn]);

  const jobFor = (key: string) => [...jobs].reverse().find((j) => j.linkKey === key);
  const filesOf = (key: string): FoundFile[] => (finds[key]?.state === "found" ? finds[key].files : []);
  const chosen = (key: string): FoundFile | undefined => {
    const files = filesOf(key);
    const p = pick[key];
    if (p === "") return undefined;
    return files.find((f) => f.path === p) ?? files[0];
  };
  const selected = songs.map((t) => keyOf(t.path)).filter((k) => !jobFor(k) && chosen(k));

  const download = async (from: Element) => {
    const rect = from.getBoundingClientRect();
    const reqs = selected.map((k) => {
      const f = chosen(k)!;
      return { username: f.username, path: f.path, size: f.size, linkKey: k, name: f.name };
    });
    try {
      const n = await api.downloadFound(reqs);
      flyToDownloads(rect, n);
      usePlayer.getState().pushToast("info", `Downloading ${n} song${n === 1 ? "" : "s"}. Each replaces its YouTube copy in your playlists when it finishes.`);
    } catch (e) {
      toastErr(e);
    }
  };

  // Above everything: the playlist page's own layers would otherwise cut this off.
  return createPortal(
    <div className="fixed inset-0 z-[120] flex items-center justify-center bg-black/50 backdrop-blur-sm animate-in" onMouseDown={onClose}>
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Scan on Soulseek"
        className="flex max-h-[80vh] w-[760px] max-w-[calc(100vw-32px)] flex-col rounded-xl bg-panel text-fg shadow-2xl ring-1 ring-line"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") onClose();
          e.stopPropagation();
        }}
      >
        <div className="flex flex-wrap items-center gap-3 border-b border-line px-5 py-4">
          <div className="min-w-0 flex-1">
            <h2 className="text-[15px] font-semibold">Scan on Soulseek</h2>
            <div className="text-[12.5px] text-muted">
              {songs.length} song{songs.length === 1 ? "" : "s"} came from YouTube (AAC ~128). Looking for the same song as FLAC, otherwise MP3 at 320 kbps.
            </div>
          </div>
          {!signedIn ? (
            <button
              onClick={() => {
                onClose();
                setView({ kind: "sources" });
              }}
              className="rounded-full px-3 py-1.5 text-[12.5px] font-medium ring-1 ring-line hover:bg-hover"
            >
              Sign in to Soulseek
            </button>
          ) : running ? (
            <>
              <span className="inline-flex items-center gap-1.5 text-[12.5px] text-muted">
                <Loader2 size={14} className="animate-spin" />
                Searching {Math.min(run.done + 1, run.total)} of {run.total}
              </span>
              <button onClick={() => api.stopFind(playlistId).catch(toastErr)} className="inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-[12.5px] font-medium ring-1 ring-line hover:bg-hover">
                <Square size={12} /> Stop
              </button>
            </>
          ) : (
            songs.length > 0 && (
              <button onClick={scan} className="inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-[12.5px] font-medium ring-1 ring-line hover:bg-hover">
                <RotateCw size={13} /> Scan again
              </button>
            )
          )}
          {selected.length > 0 && (
            <button onClick={(e) => download(e.currentTarget)} className="accent-gradient text-on-accent inline-flex items-center gap-1.5 rounded-full px-4 py-1.5 text-[12.5px] font-semibold">
              <Download size={14} /> Download {selected.length}
            </button>
          )}
          <button autoFocus aria-label="Close" onClick={onClose} className="rounded p-1 text-faint hover:text-fg">
            <X size={16} />
          </button>
        </div>
        <ul className="min-h-[120px] divide-y divide-line overflow-y-auto">
          {songs.length === 0 && <li className="px-5 py-6 text-[12.5px] text-faint">No song in this playlist came from YouTube any more.</li>}
          {songs.map((t) => {
            const k = keyOf(t.path);
            const res = finds[k];
            const job = jobFor(k);
            const f = chosen(k);
            const files = filesOf(k);
            return (
              <li key={t.path} className="flex items-start gap-3 px-5 py-2.5">
                <input
                  type="checkbox"
                  aria-label={`Download ${t.title}`}
                  title={f ? "Untick to keep the YouTube copy" : "Tick to get the recommended file"}
                  disabled={!files.length || !!job}
                  checked={!!f && !job}
                  onChange={(e) => setPick((p) => ({ ...p, [k]: e.target.checked ? (files[0]?.path ?? "") : "" }))}
                  className="mt-1 accent-[var(--c-accent)]"
                />
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[13px]" title={t.path}>
                    {t.title}
                    <span className="text-muted"> · {t.artist || "Unknown artist"}</span>
                  </div>
                  {!res && <div className="text-[11.5px] text-faint">Not searched yet</div>}
                  {res?.state === "waiting" && <div className="text-[11.5px] text-faint">Waiting…</div>}
                  {res?.state === "searching" && (
                    <div className="inline-flex items-center gap-1 text-[11.5px] text-muted">
                      <Loader2 size={11} className="animate-spin" /> Searching "{res.query}"…
                    </div>
                  )}
                  {res?.state === "skipped" && <div className="text-[11.5px] text-faint">Search cancelled.</div>}
                  {res?.state === "notFound" && <div className="text-[11.5px] text-faint">No better copy found (searched "{res.query}").</div>}
                  {res?.state === "failed" && <div className="text-[11.5px] text-warn">{res.error}</div>}
                  {files.length > 0 && (
                    <div className="mt-1 space-y-0.5">
                      {files.map((file, i) => {
                        const on = f?.path === file.path;
                        return (
                          <button
                            key={file.username + file.path}
                            type="button"
                            role="radio"
                            aria-checked={on}
                            disabled={!!job}
                            title={on ? "Click again to choose none" : file.path}
                            onClick={() => setPick((p) => ({ ...p, [k]: on ? "" : file.path }))}
                            className={`flex w-full min-w-0 items-center gap-2 rounded-md px-1.5 py-0.5 text-left text-[11.5px] transition-colors hover:bg-hover disabled:cursor-default ${on ? "text-fg" : "text-muted"}`}
                          >
                            <span className={`flex h-3.5 w-3.5 shrink-0 items-center justify-center rounded-full ring-1 ${on ? "bg-accent ring-accent" : "ring-faint"}`}>
                              {on && <span className="h-1.5 w-1.5 rounded-full bg-bg" />}
                            </span>
                            <FormatBadge f={file} />
                            <span className="min-w-0 truncate">{file.name}</span>
                            {i === 0 && <span className="shrink-0 rounded-full bg-accent/15 px-2 py-0.5 text-[10.5px] font-semibold text-accent ring-1 ring-accent/30">Recommended</span>}
                            <span className="ml-auto shrink-0 pl-2 text-faint">
                              {mb(file.size)} · {file.username} · {file.why}
                            </span>
                          </button>
                        );
                      })}
                    </div>
                  )}
                </div>
                {job && <JobStatus job={job} />}
              </li>
            );
          })}
        </ul>
        <div className="border-t border-line px-5 py-2.5 text-[11.5px] text-faint">
          A finished download replaces the YouTube copy in every playlist. The YouTube file stays on your disk.
        </div>
      </div>
    </div>,
    document.body,
  );
}
