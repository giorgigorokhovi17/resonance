import { open } from "@tauri-apps/plugin-dialog";
import { ArrowLeft, Ban, Check, ChevronDown, ChevronRight, FileOutput, FileSearch, ListChecks, Play, RotateCcw, SkipForward } from "lucide-react";
import { useCallback, useEffect, useState, type ReactNode } from "react";
import { Cover } from "../components/Cover";
import { SecondaryButton } from "../components/EmptyState";
import { ViewHeader } from "../components/ViewHeader";
import { api } from "../lib/api";
import { fmtTime } from "../lib/format";
import type { MatchCandidate, MatchDecision, MatchReason, PlaylistReview, ReviewItem } from "../lib/types";
import { liveCoverId, useLibrary } from "../store/library";
import { usePlayer } from "../store/player";
import { usePlaylists } from "../store/playlists";
import { useUi } from "../store/ui";
import { exportUnmatchedReport } from "./PlaylistView";
import { SoulseekMissing } from "./SoulseekMissing";

const toastErr = (e: unknown) => usePlayer.getState().pushToast("error", String(e));

/** Formats the player can open (matches `AUDIO_EXTENSIONS` in model.rs). */
const AUDIO_EXTENSIONS = ["flac", "wav", "wave", "mp3", "m4a", "mp4", "aac", "alac", "ogg", "oga", "opus", "aif", "aiff", "aifc", "caf", "mka"];

const REASONS: Record<MatchReason, { label: string; tone: "warn" | "accent" | "muted" }> = {
  none: { label: "Matched", tone: "muted" },
  noCandidate: { label: "No match", tone: "muted" },
  ambiguous: { label: "Several fit", tone: "accent" },
  durationMismatch: { label: "Duration differs", tone: "warn" },
  versionMismatch: { label: "Different version", tone: "warn" },
  drift: { label: "Remaster / explicit", tone: "accent" },
  lowConfidence: { label: "Not sure", tone: "muted" },
  markedMissing: { label: "Not in library", tone: "muted" },
  skipped: { label: "Skipped", tone: "muted" },
};

function ReasonBadge({ reason }: { reason: MatchReason }) {
  const r = REASONS[reason];
  const tone =
    r.tone === "warn"
      ? "bg-warn/15 text-warn ring-warn/30"
      : r.tone === "accent"
        ? "bg-accent/15 text-accent ring-accent/30"
        : "bg-hover text-muted ring-line";
  return <span className={`shrink-0 rounded-full px-2 py-0.5 text-[11px] font-medium ring-1 ${tone}`}>{r.label}</span>;
}

function SmallButton({ children, onClick, disabled, title }: { children: ReactNode; onClick: () => void; disabled?: boolean; title?: string }) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      title={title}
      className="inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-[12.5px] font-medium text-fg ring-1 ring-line transition-colors hover:bg-hover disabled:opacity-40"
    >
      {children}
    </button>
  );
}

function fileName(path: string) {
  return path.split(/[\\/]/).pop() ?? path;
}

function CandidateRow({
  c,
  rank,
  refSecs,
  busy,
  onUse,
}: {
  c: MatchCandidate;
  rank: number;
  refSecs: number | null;
  busy: boolean;
  onUse: () => void;
}) {
  const track = useLibrary((s) => (c.trackId > 0 ? s.byId.get(c.trackId) : s.byPath.get(c.path)));
  const delta = refSecs != null && c.duration > 0 ? c.duration - refSecs : null;
  const pct = Math.round(c.score * 100);
  return (
    <div className="group flex items-center gap-3 rounded-lg px-2 py-1.5 transition-colors hover:bg-hover">
      <span className="tabular w-4 shrink-0 text-right text-[11.5px] text-faint">{rank}</span>
      <div className="relative shrink-0">
        <Cover coverId={liveCoverId(track)} className="h-10 w-10" rounded="rounded" iconSize={16} />
        <button
          aria-label={`Play ${c.title || fileName(c.path)}`}
          title="Play this file (replaces the queue)"
          onClick={() => api.playPaths([c.path], 0).catch(toastErr)}
          className="absolute inset-0 flex items-center justify-center rounded bg-black/55 text-white opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100"
        >
          <Play size={15} fill="currentColor" strokeWidth={0} />
        </button>
      </div>
      <div className="min-w-0 flex-1" title={c.path}>
        <div className="truncate text-[13px]">
          {c.title || fileName(c.path)}
          {c.artist && <span className="text-muted"> · {c.artist}</span>}
        </div>
        <div className="truncate text-[11.5px] text-muted">{c.why}</div>
      </div>
      <div className="tabular shrink-0 text-right text-[12px] text-muted">
        {c.duration > 0 ? fmtTime(c.duration) : ""}
        {delta != null && Math.abs(delta) >= 0.5 && (
          <span className={Math.abs(delta) > 3 ? "ml-1 text-warn" : "ml-1 text-faint"}>
            {delta > 0 ? "+" : "−"}
            {Math.abs(delta).toFixed(0)} s
          </span>
        )}
      </div>
      <div className="hidden w-16 shrink-0 items-center gap-1.5 sm:flex" title={`Match score ${pct}%`}>
        <div className="h-1 flex-1 overflow-hidden rounded-full bg-hover">
          <div className="h-full rounded-full bg-accent" style={{ width: `${pct}%` }} />
        </div>
        <span className="tabular w-7 text-right text-[11px] text-muted">{pct}%</span>
      </div>
      <button
        onClick={onUse}
        disabled={busy}
        className="inline-flex shrink-0 items-center gap-1 rounded-full bg-accent px-3 py-1 text-[12px] font-semibold text-on-accent transition-transform hover:scale-[1.03] active:scale-[0.98] disabled:opacity-40"
      >
        <Check size={13} strokeWidth={2.5} /> Use this
      </button>
    </div>
  );
}

function RefSummary({ item }: { item: ReviewItem }) {
  const r = item.reference;
  const secs = r.durationMs != null ? r.durationMs / 1000 : null;
  const details = [r.artists.join(", ") || "Unknown artist", r.album, secs != null ? fmtTime(secs) : ""].filter(Boolean).join(" · ");
  return (
    <div className="flex items-start gap-3">
      <span className="tabular mt-0.5 w-12 shrink-0 text-[12px] text-muted" title="Position in the playlist">
        #{item.positions.join(", #")}
      </span>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate text-[14.5px] font-semibold">{r.title || "(no title)"}</span>
          <ReasonBadge reason={item.result.reason} />
        </div>
        <div className="truncate text-[12.5px] text-muted">{details}</div>
        <p className="mt-1.5 text-[12.5px] leading-snug text-fg/80">{item.result.explanation}</p>
        {r.location && (
          <div className="mt-0.5 truncate text-[11.5px] text-faint" title={r.location}>
            From: {r.location}
          </div>
        )}
      </div>
    </div>
  );
}

function ReviewCard({ item, busy, decide }: { item: ReviewItem; busy: boolean; decide: (d: MatchDecision, path?: string) => void }) {
  const refSecs = item.reference.durationMs != null ? item.reference.durationMs / 1000 : null;
  const chooseFile = async () => {
    const picked = await open({ title: `Choose the file for "${item.reference.title}"`, multiple: false, filters: [{ name: "Audio", extensions: AUDIO_EXTENSIONS }] });
    if (typeof picked === "string") decide("accept", picked);
  };
  return (
    <section className="rounded-xl bg-panel/70 p-4 ring-1 ring-line animate-in" aria-label={`Review ${item.reference.title}`}>
      <RefSummary item={item} />
      <div className="mt-3 space-y-0.5 pl-12">
        <div className="px-2 pb-1 text-[11px] font-semibold uppercase tracking-wide text-faint">Candidates in your library</div>
        {item.result.candidates.map((c, i) => (
          <CandidateRow key={c.path} c={c} rank={i + 1} refSecs={refSecs} busy={busy} onUse={() => decide("accept", c.path)} />
        ))}
      </div>
      <div className="mt-3 flex flex-wrap justify-end gap-2">
        <SmallButton onClick={chooseFile} disabled={busy} title="Pick any audio file on this PC">
          <FileSearch size={14} /> Choose a file…
        </SmallButton>
        <SmallButton onClick={() => decide("skip")} disabled={busy} title="Leave it for now; it won't be offered again">
          <SkipForward size={14} /> Skip
        </SmallButton>
        <SmallButton onClick={() => decide("missing")} disabled={busy} title="You don't have this song; it won't be offered again">
          <Ban size={14} /> Not in my library
        </SmallButton>
      </div>
    </section>
  );
}

function DecidedRow({ item, busy, undo }: { item: ReviewItem; busy: boolean; undo: () => void }) {
  const m = item.result;
  const what =
    m.reason === "markedMissing" ? "Not in your library" : m.reason === "skipped" ? "Skipped" : m.accepted ? `Using ${fileName(m.accepted.path)}` : m.explanation;
  return (
    <div className="flex items-center gap-3 rounded-lg px-3 py-2 hover:bg-hover">
      <span className="tabular w-12 shrink-0 text-[12px] text-muted">#{item.positions.join(", #")}</span>
      <div className="min-w-0 flex-1">
        <div className="truncate text-[13px]">
          {item.reference.title || "(no title)"}
          <span className="text-muted"> · {item.reference.artists.join(", ") || "Unknown artist"}</span>
        </div>
        <div className="truncate text-[11.5px] text-muted" title={m.accepted?.path}>
          {what}
        </div>
      </div>
      <SmallButton onClick={undo} disabled={busy} title="Forget this decision and match it again">
        <RotateCcw size={13} /> Undo
      </SmallButton>
    </div>
  );
}

export function ReviewView({ id }: { id: number }) {
  const version = usePlaylists((s) => s.version);
  const playlist = usePlaylists((s) => s.playlists.find((p) => p.id === id));
  const libVersion = useLibrary((s) => s.byId);
  const setView = useUi((s) => s.setView);
  const [data, setData] = useState<PlaylistReview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [showDecided, setShowDecided] = useState(false);

  useEffect(() => {
    let alive = true;
    api
      .getPlaylistReview(id)
      .then((d) => alive && setData(d))
      .catch((e) => alive && setError(String(e)));
    return () => {
      alive = false;
    };
  }, [id, version, libVersion]);

  const decide = useCallback(async (item: ReviewItem, decision: MatchDecision, path?: string) => {
    const key = item.result.refKey;
    setBusy(key);
    try {
      const n = await api.decideMatch(key, decision, path ?? null);
      const title = item.reference.title || "this song";
      const msg =
        decision === "accept"
          ? `Using ${fileName(path ?? "")} for "${title}"${n > 1 ? ` in ${n} playlists` : ""}.`
          : decision === "missing"
            ? `"${title}" marked as not in your library.`
            : decision === "skip"
              ? `Skipped "${title}".`
              : `"${title}" is back in review.`;
      usePlayer.getState().pushToast("info", msg);
      // Drop it locally right away; the playlists:changed event reloads the full state.
      setData((d) =>
        d && decision !== "clear" ? { ...d, pending: d.pending.filter((i) => i.result.refKey !== key), decided: [item, ...d.decided] } : d,
      );
    } catch (e) {
      toastErr(e);
    } finally {
      setBusy(null);
    }
  }, []);

  const back = () => setView({ kind: "playlist", id });
  const name = playlist?.name ?? "Playlist";
  const subtitle = data && (
    <>
      <button onClick={back} className="hover:text-fg hover:underline">
        {name}
      </button>
      {" · "}
      {data.pending.length} need{data.pending.length === 1 ? "s" : ""} a decision · {data.matched} matched automatically
      {data.unmatched > 0 && ` · ${data.unmatched} with no match in your library`}
    </>
  );

  return (
    <div className="h-full overflow-y-auto">
      <div className="px-6 pt-4">
        <button onClick={back} className="inline-flex items-center gap-1.5 rounded-full px-2 py-1 text-[12.5px] text-muted transition-colors hover:bg-hover hover:text-fg">
          <ArrowLeft size={14} /> Back to playlist
        </button>
      </div>
      <ViewHeader
        title="Review matches"
        subtitle={subtitle}
        actions={
          playlist && (
            <SecondaryButton onClick={() => exportUnmatchedReport(playlist)} title="CSV of everything not matched, with reasons">
              <FileOutput size={15} /> Export unmatched report
            </SecondaryButton>
          )
        }
      />
      <div className="mx-auto max-w-5xl space-y-3 px-8 pb-12">
        {error && <p className="text-[13px] text-warn">{error}</p>}
        {data && playlist?.source && <SoulseekMissing playlistId={id} items={[...data.missing, ...data.pending]} />}
        {data && data.pending.length === 0 && (
          <div className="flex items-center gap-4 rounded-xl bg-panel/70 p-5 ring-1 ring-line animate-in">
            <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-hover text-ok">
              <ListChecks size={22} />
            </div>
            <div>
              <div className="text-[14.5px] font-semibold">Nothing to review</div>
              <p className="text-[12.5px] text-muted">
                Every imported song is matched, decided, or has nothing similar in your library. Songs with no match are listed in the unmatched report.
              </p>
            </div>
          </div>
        )}
        {data?.pending.map((item) => (
          <ReviewCard
            key={item.result.refKey}
            item={item}
            busy={busy === item.result.refKey}
            decide={(d, p) => decide(item, d, p)}
          />
        ))}
        {data && data.decided.length > 0 && (
          <div className="pt-4">
            <button
              onClick={() => setShowDecided((v) => !v)}
              aria-expanded={showDecided}
              className="inline-flex items-center gap-1.5 rounded-full px-2 py-1 text-[12.5px] font-medium text-muted transition-colors hover:bg-hover hover:text-fg"
            >
              {showDecided ? <ChevronDown size={14} /> : <ChevronRight size={14} />} Decided earlier ({data.decided.length})
            </button>
            {showDecided && (
              <div className="mt-1 rounded-xl bg-panel/50 p-1 ring-1 ring-line">
                {data.decided.map((item) => (
                  <DecidedRow key={item.result.refKey} item={item} busy={busy === item.result.refKey} undo={() => decide(item, "clear")} />
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
