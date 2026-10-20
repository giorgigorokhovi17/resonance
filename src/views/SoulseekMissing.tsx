import { listen } from "@tauri-apps/api/event";
import { Check, Download, Loader2, RotateCw, Search, Square, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { api } from "../lib/api";
import { fmtTime } from "../lib/format";
import type { FindProgress, FindResult, FoundFile, ReviewItem, SlskJob } from "../lib/types";
import { usePlayer } from "../store/player";
import { useSoulseek } from "../store/soulseek";
import { useUi } from "../store/ui";

const toastErr = (e: unknown) => usePlayer.getState().pushToast("error", String(e));

function mb(bytes: number) {
  return `${(bytes / 1_048_576).toFixed(bytes < 10_485_760 ? 1 : 0)} MB`;
}

function FormatBadge({ f }: { f: FoundFile }) {
  const lossless = f.format === "FLAC";
  const label = lossless ? (f.bitDepth && f.sampleRate ? `FLAC ${f.bitDepth}/${f.sampleRate / 1000}` : "FLAC") : `MP3 ${f.bitrate ?? 320}`;
  return (
    <span
      className={`shrink-0 rounded px-1.5 py-0.5 text-[10.5px] font-semibold tracking-wide ring-1 ${lossless ? "bg-ok/10 text-ok ring-ok/30" : "bg-hover text-muted ring-line"}`}
    >
      {label}
    </span>
  );
}

function JobStatus({ job }: { job: SlskJob }) {
  if (job.state === "done") return <span className="inline-flex items-center gap-1 text-[12px] text-ok"><Check size={13} /> In the playlist</span>;
  if (job.state === "failed" || job.state === "cancelled")
    return (
      <span className="inline-flex items-center gap-2 text-[12px] text-warn" title={job.error ?? undefined}>
        {job.state === "failed" ? "Failed" : "Cancelled"}
        <button onClick={() => api.slskRetry(job.id).catch(toastErr)} className="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-fg ring-1 ring-line hover:bg-hover">
          <RotateCw size={11} /> Retry
        </button>
      </span>
    );
  const pct = job.size > 0 ? Math.round((job.bytes / job.size) * 100) : 0;
  return (
    <span className="inline-flex items-center gap-2 text-[12px] text-muted">
      <span className="h-1 w-20 overflow-hidden rounded-full bg-hover">
        <span className="block h-full bg-accent" style={{ width: `${job.state === "queued" ? 0 : pct}%` }} />
      </span>
      {job.state === "queued" ? "Waiting for the user…" : `${pct}%`}
      <button aria-label="Cancel download" onClick={() => api.slskCancel(job.id).catch(toastErr)} className="rounded p-0.5 text-faint hover:text-fg">
        <X size={12} />
      </button>
    </span>
  );
}

/**
 * Songs an imported playlist is missing, found on Soulseek. Searching is automatic; downloading
 * is not: you tick the files to get. A finished download replaces its missing entry.
 */
export function SoulseekMissing({ playlistId, items }: { playlistId: number; items: ReviewItem[] }) {
  const status = useSoulseek((s) => s.status);
  const jobs = useSoulseek((s) => s.jobs);
  const setView = useUi((s) => s.setView);
  const [results, setResults] = useState<Record<string, FindResult>>({});
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  /** Chosen file per entry (path), or "" for none. */
  const [pick, setPick] = useState<Record<string, string>>({});
  const keys = useMemo(() => items.map((i) => i.result.refKey), [items]);
  const keysSig = keys.join("|");

  // Results found earlier in this session, and live progress.
  useEffect(() => {
    let alive = true;
    api
      .findResults(playlistId, keys)
      .then((r) => {
        if (!alive) return;
        setRunning(r.running);
        setResults(Object.fromEntries(r.results.map((x) => [x.refKey, x])));
      })
      .catch(() => {});
    const unsub = [
      listen<FindProgress>("slsk:find", (e) => {
        if (e.payload.playlistId !== playlistId) return;
        setRunning(e.payload.running);
        setProgress({ done: e.payload.done, total: e.payload.total });
        setResults((r) => ({ ...r, [e.payload.result.refKey]: e.payload.result }));
      }),
      listen<number>("slsk:find-done", (e) => e.payload === playlistId && setRunning(false)),
    ];
    return () => {
      alive = false;
      unsub.forEach((p) => p.then((f) => f()));
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [playlistId, keysSig]);

  const jobFor = (key: string) => [...jobs].reverse().find((j) => j.linkKey === key);
  const chosen = (key: string): FoundFile | undefined => {
    const files = results[key]?.files ?? [];
    const p = pick[key];
    if (p === "") return undefined;
    return files.find((f) => f.path === p) ?? files[0];
  };
  const selected = items
    .map((i) => i.result.refKey)
    .filter((k) => results[k]?.state === "found" && !jobFor(k) && chosen(k));

  const find = async () => {
    try {
      setProgress({ done: 0, total: await api.findMissing(playlistId) });
      setRunning(true);
    } catch (e) {
      toastErr(e);
    }
  };
  const download = async () => {
    const reqs = selected.map((k) => {
      const f = chosen(k)!;
      return { username: f.username, path: f.path, size: f.size, linkKey: k };
    });
    try {
      const n = await api.downloadFound(reqs);
      usePlayer.getState().pushToast("info", `Downloading ${n} song${n === 1 ? "" : "s"} from Soulseek. Each one goes into the playlist when it finishes.`);
    } catch (e) {
      toastErr(e);
    }
  };

  if (!items.length) return null;
  const signedIn = status.state === "signedIn";
  const found = keys.filter((k) => results[k]?.state === "found").length;

  return (
    <section className="rounded-xl bg-panel/70 ring-1 ring-line animate-in" aria-label="Missing songs on Soulseek">
      <div className="flex flex-wrap items-center gap-3 border-b border-line px-4 py-3">
        <div className="min-w-0 flex-1">
          <div className="text-[14.5px] font-semibold">Get missing songs from Soulseek</div>
          <div className="text-[12px] text-muted">
            {items.length} song{items.length === 1 ? "" : "s"} not in your library.
            {Object.keys(results).length > 0 && ` ${found} found.`} FLAC first, otherwise MP3 at 320 kbps. Nothing downloads until you choose.
          </div>
        </div>
        {!signedIn ? (
          <button onClick={() => setView({ kind: "sources" })} className="rounded-full px-3 py-1.5 text-[12.5px] font-medium ring-1 ring-line hover:bg-hover">
            Sign in to Soulseek
          </button>
        ) : running ? (
          <>
            <span className="inline-flex items-center gap-1.5 text-[12.5px] text-muted">
              <Loader2 size={14} className="animate-spin" />
              Searching {progress ? `${Math.min(progress.done + 1, progress.total)} of ${progress.total}` : "…"}
            </span>
            <button onClick={() => api.stopFind(playlistId)} className="inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-[12.5px] font-medium ring-1 ring-line hover:bg-hover">
              <Square size={12} /> Stop
            </button>
          </>
        ) : (
          <button onClick={find} className="inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-[12.5px] font-medium ring-1 ring-line hover:bg-hover">
            <Search size={14} /> {Object.keys(results).length ? "Search again" : "Find on Soulseek"}
          </button>
        )}
        {selected.length > 0 && (
          <button
            onClick={download}
            className="accent-gradient text-on-accent inline-flex items-center gap-1.5 rounded-full px-4 py-1.5 text-[12.5px] font-semibold"
          >
            <Download size={14} /> Download {selected.length}
          </button>
        )}
      </div>
      <ul className="divide-y divide-line">
        {items.map((item) => {
          const k = item.result.refKey;
          const res = results[k];
          const job = jobFor(k);
          const f = chosen(k);
          const r = item.reference;
          return (
            <li key={k} className="flex items-start gap-3 px-4 py-2.5">
              <input
                type="checkbox"
                aria-label={`Download ${r.title}`}
                disabled={res?.state !== "found" || !!job}
                checked={!!f && res?.state === "found" && !job}
                onChange={(e) => setPick((p) => ({ ...p, [k]: e.target.checked ? (res?.files[0]?.path ?? "") : "" }))}
                className="mt-1 accent-[var(--c-accent)]"
              />
              <div className="min-w-0 flex-1">
                <div className="truncate text-[13px]">
                  {r.title || "(no title)"}
                  <span className="text-muted"> · {r.artists.join(", ") || "Unknown artist"}</span>
                  {r.durationMs != null && <span className="tabular text-faint"> · {fmtTime(r.durationMs / 1000)}</span>}
                </div>
                {!res && <div className="text-[11.5px] text-faint">Not searched yet</div>}
                {res?.state === "waiting" && <div className="text-[11.5px] text-faint">Waiting…</div>}
                {res?.state === "searching" && (
                  <div className="inline-flex items-center gap-1 text-[11.5px] text-muted">
                    <Loader2 size={11} className="animate-spin" /> Searching "{res.query}"…
                  </div>
                )}
                {res?.state === "notFound" && <div className="text-[11.5px] text-faint">No good file found (searched "{res.query}").</div>}
                {res?.state === "failed" && <div className="text-[11.5px] text-warn">{res.error}</div>}
                {res?.state === "found" && (
                  <div className="mt-0.5 space-y-0.5">
                    {res.files.map((file) => (
                      <label key={file.path} className={`flex cursor-pointer items-center gap-2 text-[11.5px] ${f?.path === file.path ? "text-fg" : "text-muted"}`} title={file.path}>
                        <input
                          type="radio"
                          name={`pick-${k}`}
                          disabled={!!job}
                          checked={f?.path === file.path}
                          onChange={() => setPick((p) => ({ ...p, [k]: file.path }))}
                          className="accent-[var(--c-accent)]"
                        />
                        <FormatBadge f={file} />
                        <span className="min-w-0 truncate">{file.name}</span>
                        <span className="shrink-0 text-faint">
                          {mb(file.size)} · {file.username} · {file.why}
                        </span>
                      </label>
                    ))}
                  </div>
                )}
              </div>
              {job && <JobStatus job={job} />}
            </li>
          );
        })}
      </ul>
    </section>
  );
}
