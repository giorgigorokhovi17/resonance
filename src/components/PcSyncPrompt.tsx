import { listen } from "@tauri-apps/api/event";
import { CheckCircle2, Loader2, RefreshCw, X } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { isMobile } from "../lib/platform";
import type { PcSyncOffer, PcSyncStatus } from "../lib/types";
import { usePlayer } from "../store/player";
import { useUi } from "../store/ui";

const fmtSize = (b: number) => (b >= 1e9 ? `${(b / 1e9).toFixed(2)} GB` : `${(b / 1e6).toFixed(b < 1e7 ? 1 : 0)} MB`);

/**
 * Pops up automatically on Android when the user scans the PC's QR code (`undertone://sync?...`),
 * asking "Sync music & playlists with PC?" — one tap on Yes downloads missing songs and puts them
 * into the matching playlists.
 */
export function PcSyncPrompt() {
  const [link, setLink] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [offer, setOffer] = useState<PcSyncOffer | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<PcSyncStatus | null>(null);

  useEffect(() => {
    if (!isMobile) return;
    const check = () => {
      api
        .pcSyncTakePendingLink()
        .then((pending) => {
          if (!pending) return;
          setLink(pending);
          setOffer(null);
          setError(null);
          setLoading(true);
          const cur = usePlayer.getState().settings?.pcSync;
          if (cur && cur.link !== pending) {
            api.updateSettings({ pcSync: { ...cur, link: pending } }).then((s) => usePlayer.getState().setSettings(s)).catch(() => {});
          }
          api
            .pcSyncOffer(pending)
            .then((o) => setOffer(o))
            .catch((e) => setError(String(e)))
            .finally(() => setLoading(false));
        })
        .catch(() => {});
    };
    check();
    const onVis = () => {
      if (document.visibilityState === "visible") check();
    };
    window.addEventListener("focus", check);
    document.addEventListener("visibilitychange", onVis);
    const un = listen<PcSyncStatus>("pcsync:status", (e) => setStatus(e.payload));
    return () => {
      window.removeEventListener("focus", check);
      document.removeEventListener("visibilitychange", onVis);
      un.then((f) => f());
    };
  }, []);

  if (!link) return null;

  const running = !!status?.running;
  const doneSummary = !running ? status?.summary : null;
  const allLists = offer?.lists ?? [];
  const realPlaylists = allLists.filter((l) => l.id !== 0);
  const allSongsEntry = allLists.find((l) => l.id === 0);
  const newCount = allSongsEntry ? allSongsEntry.newCount : Math.max(0, (offer?.totalPcSongs ?? 0) - (offer?.totalOnPhone ?? 0));
  const newBytes = allSongsEntry ? allSongsEntry.newBytes : realPlaylists.reduce((a, l) => a + l.newBytes, 0);
  const pct = status?.total ? Math.min(100, Math.round(((status.done + 1) / status.total) * 100)) : 0;

  const close = () => {
    setLink(null);
    setOffer(null);
    setError(null);
  };

  const syncAll = async () => {
    if (!offer || !link) return;
    setError(null);
    const cur = usePlayer.getState().settings?.pcSync ?? { link, lists: [], folders: [], send: false };
    // Pick All songs + every playlist so every song comes over and goes straight into its PC playlists.
    const next = { ...cur, link, lists: offer.lists.map((l) => l.id) };
    try {
      const updated = await api.updateSettings({ pcSync: next });
      usePlayer.getState().setSettings(updated);
      await api.pcSyncStart();
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Sync with PC"
      className="fixed inset-0 z-50 flex items-end justify-center bg-black/60 p-4 backdrop-blur-sm animate-in"
      onClick={() => !running && close()}
    >
      <div
        className="glass-card w-full max-w-md rounded-2xl bg-panel p-5 shadow-2xl ring-1 ring-line animate-np-sheet"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-start justify-between gap-3">
          <div className="flex items-center gap-2.5">
            <span className="flex h-10 w-10 items-center justify-center rounded-xl bg-accent/15 text-accent">
              <RefreshCw size={20} className={running ? "animate-spin" : ""} />
            </span>
            <div>
              <h2 className="text-[16px] font-bold">Sync music &amp; playlists?</h2>
              <p className="text-[12px] text-muted">Connected to your PC on Wi-Fi</p>
            </div>
          </div>
          <button
            aria-label="Close"
            onClick={close}
            className="flex h-8 w-8 items-center justify-center rounded-full text-faint hover:bg-hover hover:text-fg"
          >
            <X size={18} />
          </button>
        </div>

        <div className="mt-4 space-y-3">
          {loading && (
            <div className="flex items-center gap-2.5 rounded-xl bg-bg/70 p-3 text-[13px] text-muted ring-1 ring-line">
              <Loader2 size={16} className="animate-spin text-accent" />
              Checking your PC library…
            </div>
          )}

          {error && <div className="rounded-xl bg-warn/10 p-3 text-[12.5px] text-warn ring-1 ring-warn/30">{error}</div>}

          {offer && !running && !doneSummary && (
            <div className="rounded-xl bg-bg/70 p-3.5 ring-1 ring-line">
              <div className="text-[13.5px] font-medium">
                {newCount > 0 ? (
                  <>
                    {newCount} new {newCount === 1 ? "song" : "songs"} to download{" "}
                    {newBytes > 0 && <span className="text-muted">({fmtSize(newBytes)})</span>}
                  </>
                ) : (
                  <span className="text-ok">All {offer.totalPcSongs} PC songs are already on this phone</span>
                )}
              </div>
              <div className="mt-1 text-[12px] text-muted">
                {offer.totalOnPhone} already on phone · {realPlaylists.length} {realPlaylists.length === 1 ? "playlist" : "playlists"} will be synced automatically.
              </div>
            </div>
          )}

          {running && (
            <div className="space-y-2 rounded-xl bg-bg/70 p-3.5 ring-1 ring-line">
              <div className="flex items-center justify-between text-[12.5px]">
                <span className="truncate font-medium">{status?.step || "Starting sync…"}</span>
                {status?.total ? (
                  <span className="shrink-0 tabular text-muted">
                    {status.done + 1} / {status.total}
                  </span>
                ) : null}
              </div>
              <div className="h-2 overflow-hidden rounded-full bg-fg/10">
                <div className="h-full rounded-full bg-gradient-to-r from-accent to-accent2 transition-[width] duration-300" style={{ width: `${status?.total ? pct : 30}%` }} />
              </div>
            </div>
          )}

          {doneSummary && (
            <div className="flex items-center gap-2.5 rounded-xl bg-ok/10 p-3.5 text-[13px] text-ok ring-1 ring-ok/30">
              <CheckCircle2 size={18} className="shrink-0" />
              <span>
                Done! {doneSummary.fetched > 0 ? `Downloaded ${doneSummary.fetched} new ${doneSummary.fetched === 1 ? "song" : "songs"} into your playlists.` : "Your playlists are up to date."}
              </span>
            </div>
          )}
        </div>

        <div className="mt-5 flex flex-col gap-2">
          {running ? (
            <div className="flex gap-2">
              <button onClick={close} className="flex-1 rounded-xl bg-accent py-2.5 text-[13.5px] font-semibold text-on-accent active:scale-98">
                Run in background
              </button>
              <button onClick={() => api.pcSyncCancel()} className="rounded-xl px-4 py-2.5 text-[13.5px] font-medium text-muted ring-1 ring-line active:scale-98">
                Stop
              </button>
            </div>
          ) : doneSummary ? (
            <button onClick={close} className="w-full rounded-xl bg-accent py-2.5 text-[13.5px] font-semibold text-on-accent active:scale-98">
              Done
            </button>
          ) : (
            <>
              <button
                disabled={!offer || loading}
                onClick={syncAll}
                className="w-full rounded-xl bg-accent py-3 text-[14px] font-semibold text-on-accent transition-transform active:scale-98 disabled:opacity-45"
              >
                {newCount > 0 ? `Yes, sync ${newCount} new & playlists` : "Yes, sync playlists"}
              </button>
              <div className="flex gap-2">
                <button
                  onClick={() => {
                    close();
                    useUi.getState().setView({ kind: "settings" });
                  }}
                  className="flex-1 rounded-xl py-2.5 text-[13px] font-medium text-muted ring-1 ring-line active:scale-98"
                >
                  Choose playlists
                </button>
                <button onClick={close} className="flex-1 rounded-xl py-2.5 text-[13px] font-medium text-muted ring-1 ring-line active:scale-98">
                  Not now
                </button>
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
