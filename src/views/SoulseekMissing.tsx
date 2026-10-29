import { Check, Download, Layers, Loader2, RotateCw, Search, Square, X, TvMinimalPlay } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { api } from "../lib/api";
import { flyToDownloads } from "../lib/fly";
import { fmtTime } from "../lib/format";
import { YOUTUBE, type FindMode, type FoundFile, type ReviewItem, type SlskJob } from "../lib/types";
import { usePlayer } from "../store/player";
import { useSoulseek } from "../store/soulseek";
import { useUi } from "../store/ui";
import { OtherVersions, type PickedVersion } from "./OtherVersions";

const toastErr = (e: unknown) => usePlayer.getState().pushToast("error", String(e));

export function mb(bytes: number) {
  return `${(bytes / 1_048_576).toFixed(bytes < 10_485_760 ? 1 : 0)} MB`;
}

export function FormatBadge({ f }: { f: FoundFile }) {
  if (f.username === YOUTUBE)
    return <span className="shrink-0 rounded bg-warn/10 px-1.5 py-0.5 text-[10.5px] font-semibold tracking-wide text-warn ring-1 ring-warn/30">AAC ~128</span>;
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

function Tag({ children, tone }: { children: string; tone: "accent" | "muted" }) {
  return (
    <span
      className={`shrink-0 rounded-full px-2 py-0.5 text-[10.5px] font-semibold ${tone === "accent" ? "bg-accent/15 text-accent ring-1 ring-accent/30" : "bg-hover text-muted ring-1 ring-line"}`}
    >
      {children}
    </span>
  );
}

export function JobStatus({ job }: { job: SlskJob }) {
  if (job.state === "done")
    return (
      <span className="inline-flex shrink-0 items-center gap-1 text-[12px] text-ok">
        <Check size={13} /> {job.linkKey ? "In the playlist" : "Done"}
      </span>
    );
  if (job.state === "failed" || job.state === "cancelled")
    return (
      <span className="inline-flex shrink-0 items-center gap-2 text-[12px] text-warn" title={job.error ?? undefined}>
        {job.state === "failed" ? "Failed" : "Cancelled"}
        <button onClick={() => api.slskRetry(job.id).catch(toastErr)} className="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-fg ring-1 ring-line hover:bg-hover">
          <RotateCw size={11} /> Retry
        </button>
      </span>
    );
  const pct = job.size > 0 ? Math.round((job.bytes / job.size) * 100) : 0;
  return (
    <span className="inline-flex shrink-0 items-center gap-2 text-[12px] text-muted">
      <span className="h-1 w-20 overflow-hidden rounded-full bg-hover">
        <span className="block h-full bg-accent transition-[width]" style={{ width: `${job.state === "queued" ? 0 : pct}%` }} />
      </span>
      <span className="tabular w-[110px]">
        {job.state === "queued" ? (job.username === YOUTUBE ? "Starting…" : "Waiting for the user…") : `${pct}% · ${mb(job.speed)}/s`}
      </span>
      <button aria-label="Cancel download" title="Cancel" onClick={() => api.slskCancel(job.id).catch(toastErr)} className="rounded p-0.5 text-faint hover:text-fg">
        <X size={12} />
      </button>
    </span>
  );
}

/**
 * Songs an imported playlist is missing, found on Soulseek (or YouTube, for what Soulseek doesn't
 * have). Searching is automatic; downloading is not: you choose a file per song (or none), unless
 * you ask for everything from YouTube. A finished download replaces its missing entry.
 */
export function SoulseekMissing({ playlistId, items }: { playlistId: number; items: ReviewItem[] }) {
  const status = useSoulseek((s) => s.status);
  const yt = useSoulseek((s) => s.yt);
  const jobs = useSoulseek((s) => s.jobs);
  const finds = useSoulseek((s) => s.finds);
  const run = useSoulseek((s) => s.findRuns[playlistId]);
  const setView = useUi((s) => s.setView);
  /** Chosen file per song: a path, "" for none. Unset = the recommended one. */
  const [pick, setPick] = useState<Record<string, string>>({});
  /** A file of another version chosen in "Other versions", per song. */
  const [picked, setPicked] = useState<Record<string, PickedVersion>>({});
  const [versionsOf, setVersionsOf] = useState<ReviewItem | null>(null);
  const keys = useMemo(() => items.map((i) => i.result.refKey), [items]);
  const keysSig = keys.join("|");

  // Results found earlier in this session (the store only has ones seen since the app started).
  useEffect(() => {
    api
      .findResults(playlistId, keys)
      .then((r) =>
        useSoulseek.setState((s) => ({
          finds: { ...Object.fromEntries(r.results.map((x) => [x.refKey, { ...x, playlistId }])), ...s.finds },
          findRuns: s.findRuns[playlistId] || !r.running ? s.findRuns : { ...s.findRuns, [playlistId]: { done: 0, total: keys.length, running: true } },
        })),
      )
      .catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [playlistId, keysSig]);

  const running = !!run?.running;
  const jobFor = (key: string) => [...jobs].reverse().find((j) => j.linkKey === key);
  const filesOf = (key: string): FoundFile[] => {
    const files = finds[key]?.state === "found" ? finds[key].files : [];
    const other = picked[key];
    return other && !files.some((f) => f.username === other.username && f.path === other.path) ? [...files, other] : files;
  };
  const chosen = (key: string): FoundFile | undefined => {
    const files = filesOf(key);
    const p = pick[key];
    if (p === "") return undefined;
    return files.find((f) => f.path === p) ?? files[0];
  };
  const selected = keys.filter((k) => filesOf(k).length > 0 && !jobFor(k) && chosen(k));

  const find = async (mode: FindMode = "soulseekFirst", from?: Element) => {
    const rect = from?.getBoundingClientRect();
    try {
      const total = await api.findMissing(playlistId, null, mode);
      // "Get all from YouTube" queues the songs straight away.
      if (mode === "youTube") flyToDownloads(rect, total);
      useSoulseek.setState((s) => ({ findRuns: { ...s.findRuns, [playlistId]: { done: 0, total, running: true } } }));
    } catch (e) {
      toastErr(e);
    }
  };
  const download = async (from: Element) => {
    const rect = from.getBoundingClientRect();
    const reqs = selected.map((k) => {
      const f = chosen(k)!;
      return { username: f.username, path: f.path, size: f.size, linkKey: k, name: f.name };
    });
    try {
      const n = await api.downloadFound(reqs);
      flyToDownloads(rect, n);
      usePlayer.getState().pushToast("info", `Downloading ${n} song${n === 1 ? "" : "s"}. Follow them with the Downloads button; each goes into the playlist when it finishes.`);
    } catch (e) {
      toastErr(e);
    }
  };

  if (!items.length) return null;
  const signedIn = status.state === "signedIn";
  const ytReady = yt.enabled && yt.state === "ready";
  const found = keys.filter((k) => finds[k]?.state === "found").length;
  const searched = keys.some((k) => finds[k]);
  const canSearch = signedIn || ytReady;

  return (
    <section className="rounded-xl bg-panel/70 ring-1 ring-line animate-in" aria-label="Missing songs">
      <div className="flex flex-wrap items-center gap-3 border-b border-line px-4 py-3">
        <div className="min-w-0 flex-1">
          <div className="text-[14.5px] font-semibold">Get missing songs</div>
          <div className="text-[12px] text-muted">
            {items.length} song{items.length === 1 ? "" : "s"} not in your library.
            {searched && ` ${found} found.`} {signedIn ? "Soulseek first: FLAC, otherwise MP3 at 320 kbps" : "Soulseek isn't signed in"}
            {ytReady ? "; YouTube for the rest." : "."} Nothing downloads until you choose.
          </div>
        </div>
        {!signedIn && !ytReady ? (
          <button onClick={() => setView({ kind: "sources" })} className="rounded-full px-3 py-1.5 text-[12.5px] font-medium ring-1 ring-line hover:bg-hover">
            Sign in to Soulseek or turn on YouTube
          </button>
        ) : running ? (
          <>
            <span className="inline-flex items-center gap-1.5 text-[12.5px] text-muted">
              <Loader2 size={14} className="animate-spin" />
              Searching {run ? `${Math.min(run.done + 1, run.total)} of ${run.total}` : "…"}
            </span>
            <button onClick={() => api.stopFind(playlistId)} className="inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-[12.5px] font-medium ring-1 ring-line hover:bg-hover">
              <Square size={12} /> Stop all
            </button>
          </>
        ) : (
          <>
            <button onClick={() => find()} className="inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-[12.5px] font-medium ring-1 ring-line hover:bg-hover">
              <Search size={14} /> {searched ? "Search again" : signedIn ? "Find on Soulseek" : "Find on YouTube"}
            </button>
            {ytReady && (
              <button
                onClick={(e) => find("youTube", e.currentTarget)}
                title="Download every missing song from YouTube now, at YouTube's quality"
                className="inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-[12.5px] font-medium ring-1 ring-line hover:bg-hover"
              >
                <TvMinimalPlay size={14} /> Get all from YouTube
              </button>
            )}
          </>
        )}
        {selected.length > 0 && (
          <button onClick={(e) => download(e.currentTarget)} className="accent-gradient text-on-accent inline-flex items-center gap-1.5 rounded-full px-4 py-1.5 text-[12.5px] font-semibold">
            <Download size={14} /> Download {selected.length}
          </button>
        )}
      </div>
      <ul className="divide-y divide-line">
        {items.map((item) => {
          const k = item.result.refKey;
          const res = finds[k];
          const job = jobFor(k);
          const f = chosen(k);
          const r = item.reference;
          const files = filesOf(k);
          const other = picked[k];
          const smallest = files.length > 1 ? files.reduce((a, b) => (b.size < a.size ? b : a)) : undefined;
          const cancellable = running && (res?.state === "waiting" || res?.state === "searching");
          return (
            <li key={k} className="flex items-start gap-3 px-4 py-2.5">
              <input
                type="checkbox"
                aria-label={`Download ${r.title}`}
                title={f ? "Untick to get none of these" : "Tick to get the recommended file"}
                disabled={!files.length || !!job}
                checked={!!f && !job}
                onChange={(e) => setPick((p) => ({ ...p, [k]: e.target.checked ? (files[0]?.path ?? "") : "" }))}
                className="mt-1 accent-[var(--c-accent)]"
              />
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <div className="min-w-0 truncate text-[13px]">
                    {r.title || "(no title)"}
                    <span className="text-muted"> · {r.artists.join(", ") || "Unknown artist"}</span>
                    {r.durationMs != null && <span className="tabular text-faint"> · {fmtTime(r.durationMs / 1000)}</span>}
                  </div>
                  {canSearch && !cancellable && !job && (
                    <button
                      onClick={() => setVersionsOf(item)}
                      title="Find the original, slowed, sped up, hardstyle and other versions of this song"
                      className="ml-auto inline-flex shrink-0 items-center gap-1 rounded-full px-2 py-0.5 text-[11px] text-muted ring-1 ring-line hover:bg-hover hover:text-fg"
                    >
                      <Layers size={11} /> Other versions
                    </button>
                  )}
                  {cancellable && (
                    <button
                      onClick={() => api.skipFind(k).catch(toastErr)}
                      title="Don't search for this song"
                      className="ml-auto inline-flex shrink-0 items-center gap-1 rounded-full px-2 py-0.5 text-[11px] text-muted ring-1 ring-line hover:bg-hover hover:text-fg"
                    >
                      <X size={11} /> Cancel search
                    </button>
                  )}
                </div>
                {!res && <div className="text-[11.5px] text-faint">Not searched yet</div>}
                {res?.state === "waiting" && <div className="text-[11.5px] text-faint">Waiting…</div>}
                {res?.state === "searching" && (
                  <div className="inline-flex items-center gap-1 text-[11.5px] text-muted">
                    <Loader2 size={11} className="animate-spin" /> Searching "{res.query}"…
                  </div>
                )}
                {res?.state === "skipped" && <div className="text-[11.5px] text-faint">Search cancelled.</div>}
                {res?.state === "notFound" && <div className="text-[11.5px] text-faint">No good file found (searched "{res.query}").</div>}
                {res?.state === "failed" && <div className="text-[11.5px] text-warn">{res.error}</div>}
                {files.length > 0 && (
                  <div className="mt-1 space-y-0.5">
                    {files.map((file, i) => {
                      const on = f?.path === file.path;
                      return (
                        <button
                          key={file.path}
                          type="button"
                          role="radio"
                          aria-checked={on}
                          disabled={!!job}
                          title={on ? "Click again to choose none" : file.path}
                          // Clicking the chosen file again leaves the song out.
                          onClick={() => setPick((p) => ({ ...p, [k]: on ? "" : file.path }))}
                          className={`flex w-full min-w-0 items-center gap-2 rounded-md px-1.5 py-0.5 text-left text-[11.5px] transition-colors hover:bg-hover disabled:cursor-default ${on ? "text-fg" : "text-muted"}`}
                        >
                          <span className={`flex h-3.5 w-3.5 shrink-0 items-center justify-center rounded-full ring-1 ${on ? "bg-accent ring-accent" : "ring-faint"}`}>
                            {on && <span className="h-1.5 w-1.5 rounded-full bg-bg" />}
                          </span>
                          <FormatBadge f={file} />
                          <span className="min-w-0 truncate">{file.name}</span>
                          {other && file.path === other.path && file.username === other.username ? (
                            <Tag tone="muted">{other.version}</Tag>
                          ) : (
                            i === 0 && res?.state === "found" && <Tag tone="accent">Recommended</Tag>
                          )}
                          {smallest && file.path === smallest.path && file.username === smallest.username && i !== 0 && <Tag tone="muted">Smallest</Tag>}
                          <span className="shrink-0 text-faint">
                            {mb(file.size)} · {file.username} · {file.why}
                          </span>
                        </button>
                      );
                    })}
                    {!f && !job && <div className="pl-6 text-[11px] text-faint">None chosen. This song won't be downloaded.</div>}
                  </div>
                )}
              </div>
              {job && <JobStatus job={job} />}
            </li>
          );
        })}
      </ul>
      {versionsOf && (
        <OtherVersions
          playlistId={playlistId}
          item={versionsOf}
          onClose={() => setVersionsOf(null)}
          onPick={(f) => {
            const key = versionsOf.result.refKey;
            setPicked((p) => ({ ...p, [key]: f }));
            setPick((p) => ({ ...p, [key]: f.path }));
            setVersionsOf(null);
          }}
        />
      )}
    </section>
  );
}
