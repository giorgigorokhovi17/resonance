import { Loader2, RotateCw, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api } from "../lib/api";
import { fmtTime } from "../lib/format";
import type { ReviewItem, VersionFile, VersionsResult } from "../lib/types";
import { FormatBadge, mb } from "./SoulseekMissing";

function lengthText(d: number | null) {
  if (d == null) return "length unknown";
  if (Math.abs(d) <= 3) return "same length";
  return `${d > 0 ? "+" : "−"}${fmtTime(Math.abs(d))} vs. the playlist`;
}

/** A file of another version, with the version's name, once chosen in the popup. */
export type PickedVersion = VersionFile & { version: string };

/**
 * Other versions of one missing song (original, slowed, hardstyle, …) on Soulseek and YouTube,
 * grouped by version. Choosing one makes it the song's pick; nothing downloads from here.
 */
export function OtherVersions({
  playlistId,
  item,
  onPick,
  onClose,
}: {
  playlistId: number;
  item: ReviewItem;
  onPick: (f: PickedVersion) => void;
  onClose: () => void;
}) {
  const k = item.result.refKey;
  const r = item.reference;
  const [res, setRes] = useState<VersionsResult | null>(null);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Only the latest search's answer counts (an earlier one ends cancelled, with partial results).
  const latest = useRef(0);
  const search = (fresh: boolean) => {
    const id = ++latest.current;
    setBusy(true);
    setError(null);
    api
      .findVersions(playlistId, k, fresh)
      .then((r) => id === latest.current && setRes(r))
      .catch((e) => id === latest.current && setError(String(e)))
      .finally(() => id === latest.current && setBusy(false));
  };
  useEffect(() => {
    search(false);
    return () => void api.cancelVersions().catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [k]);

  const rec = res?.recommended;
  return (
    <div className="fixed inset-0 z-[120] flex items-center justify-center bg-black/50 backdrop-blur-sm animate-in" onMouseDown={onClose}>
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Other versions"
        className="flex max-h-[80vh] w-[720px] max-w-[calc(100vw-32px)] flex-col rounded-xl bg-panel shadow-2xl ring-1 ring-line"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") onClose();
          e.stopPropagation();
        }}
      >
        <div className="flex items-start gap-3 border-b border-line px-5 py-4">
          <div className="min-w-0 flex-1">
            <h2 className="text-[15px] font-semibold">Other versions</h2>
            <div className="truncate text-[12.5px] text-muted">
              {r.title} · {r.artists.join(", ") || "Unknown artist"}
              {r.durationMs != null && ` · ${fmtTime(r.durationMs / 1000)}`}
            </div>
          </div>
          {!busy && (
            <button onClick={() => search(true)} className="inline-flex shrink-0 items-center gap-1 rounded-full px-3 py-1 text-[12px] ring-1 ring-line hover:bg-hover">
              <RotateCw size={12} /> Search again
            </button>
          )}
          <button autoFocus aria-label="Close" onClick={onClose} className="rounded p-1 text-faint hover:text-fg">
            <X size={16} />
          </button>
        </div>
        <div className="min-h-[120px] overflow-y-auto px-5 py-3">
          {busy && (
            <div className="flex items-center gap-2 py-6 text-[12.5px] text-muted">
              <Loader2 size={14} className="animate-spin" /> Searching Soulseek and YouTube. This takes about 20 seconds.
            </div>
          )}
          {!busy && error && <div className="py-6 text-[12.5px] text-warn">{error}</div>}
          {!busy && res && !res.groups.length && (
            <div className="py-6 text-[12.5px] text-faint">{res.error ?? `No other versions found (searched "${res.queries.join('", "')}").`}</div>
          )}
          {!busy &&
            res?.groups.map((g) => (
              <section key={g.key} className="py-2">
                <div className="mb-1 flex items-center gap-2 text-[12px] font-semibold uppercase tracking-wider text-muted">
                  {g.label}
                  {g.requested && <span className="rounded-full bg-hover px-2 py-0.5 text-[10.5px] normal-case tracking-normal text-muted ring-1 ring-line">The playlist's version</span>}
                </div>
                <div className="space-y-0.5">
                  {g.files.map((f) => {
                    const isRec = !!rec && rec[0] === f.username && rec[1] === f.path;
                    return (
                      <button
                        key={f.username + f.path}
                        type="button"
                        title={f.path}
                        onClick={() => onPick({ ...f, version: g.label })}
                        className="flex w-full min-w-0 items-center gap-2 rounded-md px-1.5 py-1 text-left text-[12px] text-muted transition-colors hover:bg-hover hover:text-fg"
                      >
                        <FormatBadge f={f} />
                        <span className="min-w-0 truncate text-fg">{f.name}</span>
                        {isRec && <span className="shrink-0 rounded-full bg-accent/15 px-2 py-0.5 text-[10.5px] font-semibold text-accent ring-1 ring-accent/30">Recommended</span>}
                        {!f.sameArtist && (
                          <span className="shrink-0 rounded-full px-2 py-0.5 text-[10.5px] font-semibold text-warn ring-1 ring-warn/30" title="The playlist's artist isn't in this file's name. Check it's the right song.">
                            Different artist
                          </span>
                        )}
                        <span className="ml-auto shrink-0 pl-2 text-faint">
                          {f.size > 0 && `${mb(f.size)} · `}
                          {f.username} · {f.why} · {lengthText(f.lengthDelta)}
                        </span>
                      </button>
                    );
                  })}
                </div>
              </section>
            ))}
        </div>
        <div className="border-t border-line px-5 py-2.5 text-[11.5px] text-faint">
          Choosing a file makes it this song's pick. Nothing downloads until you press Download.
        </div>
      </div>
    </div>
  );
}
