import { Link2, Loader2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { create } from "zustand";
import { api } from "../lib/api";
import type { FindMode, LinkInfo } from "../lib/types";
import { usePlayer } from "../store/player";
import { useSoulseek } from "../store/soulseek";
import { useUi } from "../store/ui";

const useLinkDialog = create<{ open: boolean }>(() => ({ open: false }));

/** What to do with the songs the library doesn't have. */
type MissingChoice = FindMode | "later";

/** Opens the "Import from link" dialog. */
export function openLinkImport() {
  useLinkDialog.setState({ open: true });
}

/**
 * Paste a public Spotify, YouTube / YouTube Music or Apple Music playlist or album link. Songs
 * are matched to your files; the rest stay in the playlist as missing, ready to find on Soulseek.
 */
export function LinkImportHost() {
  const open = useLinkDialog((s) => s.open);
  const [url, setUrl] = useState("");
  const [info, setInfo] = useState<LinkInfo | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [missingChoice, setMissingChoice] = useState<MissingChoice>("soulseekFirst");
  const signedIn = useSoulseek((s) => s.status.state === "signedIn");
  const ytReady = useSoulseek((s) => s.yt.enabled && s.yt.state === "ready");
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!open) return;
    setUrl("");
    setInfo(null);
    setError(null);
    setBusy(false);
    setTimeout(() => inputRef.current?.focus(), 30);
  }, [open]);

  useEffect(() => {
    let alive = true;
    const t = url.trim();
    if (!t) {
      setInfo(null);
      return;
    }
    api
      .checkLink(t)
      .then((i) => alive && setInfo(i))
      .catch(() => alive && setInfo(null));
    return () => {
      alive = false;
    };
  }, [url]);

  if (!open) return null;
  const close = () => !busy && useLinkDialog.setState({ open: false });
  const submit = async () => {
    if (!info || busy) return;
    setBusy(true);
    setError(null);
    try {
      const r = await api.importLink(url.trim());
      const matched = Object.values(r.matched).reduce((a, b) => a + b, 0);
      const missing = r.total - matched;
      const parts = [`${matched} of ${r.total} songs are in your library`];
      if (missing > 0) parts.push(`${missing} missing`);
      if (r.review > 0) parts.push(`${r.review} need a decision`);
      usePlayer
        .getState()
        .pushToast(r.note ? "error" : "info", `${r.updated ? "Updated" : "Imported"} "${r.name}" from ${r.service}: ${parts.join(", ")}.${r.note ? ` ${r.note}` : ""}`);
      useLinkDialog.setState({ open: false });
      useUi.getState().setView(missing > 0 ? { kind: "review", id: r.id } : { kind: "playlist", id: r.id });
      const mode = choice(missingChoice);
      if (missing > 0 && mode) {
        try {
          const total = await api.findMissing(r.id, null, mode);
          useSoulseek.setState((s) => ({ findRuns: { ...s.findRuns, [r.id]: { done: 0, total, running: true } } }));
        } catch (e) {
          usePlayer.getState().pushToast("error", String(e));
        }
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  // A choice that isn't available right now falls back to doing nothing.
  const choice = (c: MissingChoice): FindMode | null =>
    c === "youTube" ? (ytReady ? "youTube" : null) : c === "soulseekFirst" ? (signedIn || ytReady ? "soulseekFirst" : null) : null;
  const options: { value: MissingChoice; label: string; note: string; off?: string }[] = [
    {
      value: "soulseekFirst",
      label: "Look on Soulseek first (better quality)",
      note: ytReady ? "YouTube for the rest. You choose what to download." : "You choose what to download.",
      off: signedIn || ytReady ? undefined : "Sign in to Soulseek on the Sources page first.",
    },
    {
      value: "youTube",
      label: "Download from YouTube (YouTube's quality)",
      note: "AAC at about 128 kbps. Starts right away.",
      off: ytReady ? undefined : "Turn on YouTube downloads on the Sources page first.",
    },
    { value: "later", label: "Just import", note: "Find them later from the playlist's review page." },
  ];

  const hint = !url.trim()
    ? "Spotify, YouTube, YouTube Music or Apple Music: playlists and albums. The link has to be public."
    : info
      ? `${info.service} ${info.kind}`
      : "Not a link Undertone can read. Paste a playlist or album link.";

  return (
    <div className="fixed inset-0 z-[120] flex items-center justify-center bg-black/50 backdrop-blur-sm animate-in" onMouseDown={close}>
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Import from link"
        className="w-[520px] rounded-xl bg-panel p-5 shadow-2xl ring-1 ring-line"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") close();
          if (e.key === "Enter") submit();
          e.stopPropagation();
        }}
      >
        <h2 className="flex items-center gap-2 text-[15px] font-semibold">
          <Link2 size={17} className="text-accent" /> Import from link
        </h2>
        <p className="mt-1.5 text-[12.5px] leading-relaxed text-muted">
          Songs you have are matched to your files. The rest stay in the playlist as missing until you get them.
        </p>
        <input
          ref={inputRef}
          value={url}
          disabled={busy}
          onChange={(e) => setUrl(e.target.value)}
          placeholder="https://open.spotify.com/playlist/…"
          aria-label="Playlist or album link"
          className="mt-4 h-10 w-full rounded-md bg-bg px-3 text-[13px] outline-none ring-1 ring-line focus:ring-accent disabled:opacity-60"
        />
        <div className={`mt-2 min-h-[18px] text-[12px] ${url.trim() && !info ? "text-warn" : info ? "text-ok" : "text-faint"}`}>{hint}</div>
        <fieldset className="mt-3" disabled={busy}>
          <legend className="text-[12px] font-semibold uppercase tracking-wider text-faint">Songs you don't have</legend>
          <div className="mt-1.5 space-y-1">
            {options.map((o) => (
              <label
                key={o.value}
                className={`flex items-start gap-2.5 rounded-md px-2 py-1.5 text-[12.5px] ${o.off ? "opacity-50" : "cursor-pointer hover:bg-hover"}`}
              >
                <input
                  type="radio"
                  name="missing"
                  className="mt-0.5 accent-[var(--c-accent)]"
                  checked={missingChoice === o.value}
                  disabled={!!o.off}
                  onChange={() => setMissingChoice(o.value)}
                />
                <span>
                  <span className="text-fg">{o.label}</span>
                  <span className="block text-[11.5px] text-faint">{o.off ?? o.note}</span>
                </span>
              </label>
            ))}
          </div>
        </fieldset>
        {error && <p className="mt-2 text-[12.5px] leading-snug text-warn">{error}</p>}
        <div className="mt-4 flex justify-end gap-2">
          <button onClick={close} disabled={busy} className="rounded-full px-4 py-1.5 text-[12.5px] font-medium text-muted ring-1 ring-line hover:bg-hover disabled:opacity-40">
            Cancel
          </button>
          <button
            onClick={submit}
            disabled={!info || busy}
            className="accent-gradient text-on-accent inline-flex items-center gap-1.5 rounded-full px-4 py-1.5 text-[12.5px] font-semibold disabled:opacity-40"
          >
            {busy && <Loader2 size={14} className="animate-spin" />}
            {busy ? `Reading ${info?.service ?? ""}…` : "Import"}
          </button>
        </div>
      </div>
    </div>
  );
}
