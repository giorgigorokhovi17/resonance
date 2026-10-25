import { Download, Globe, Loader2, Play, Plus, Sparkles } from "lucide-react";
import { useEffect, useState } from "react";
import { playTracks } from "../lib/actions";
import { api } from "../lib/api";
import { displayArtist, fmtTime } from "../lib/format";
import type { OnlineSuggestion, Recommendation, TrackRef } from "../lib/types";
import { liveCoverId, longEnough, useLibrary } from "../store/library";
import { usePlayer } from "../store/player";
import { usePlaylists } from "../store/playlists";
import { useUi } from "../store/ui";
import { Cover } from "./Cover";

const toastErr = (e: unknown) => usePlayer.getState().pushToast("error", String(e));

/** Online results per playlist, kept while the app runs so coming back doesn't ask Deezer again. */
const onlineCache = new Map<number, OnlineSuggestion[]>();

function toRef(s: OnlineSuggestion): TrackRef {
  return {
    title: s.title,
    artists: s.artists,
    album: s.album,
    durationMs: s.duration > 0 ? Math.round(s.duration * 1000) : null,
    isrc: null,
    mbid: null,
    serviceIds: { deezer: String(s.deezerId) },
    location: null,
  };
}

const addBtn =
  "inline-flex shrink-0 items-center gap-1 rounded-full px-2.5 py-1 text-[12px] font-medium text-muted ring-1 ring-line transition-colors hover:bg-hover hover:text-fg disabled:opacity-40";

/** "Recommended songs" under a playlist: library songs that fit, plus (on request) songs you don't have. */
export function PlaylistRecommendations({ id }: { id: number }) {
  const version = usePlaylists((s) => s.version);
  const libVersion = useLibrary((s) => s.byId);
  const minLength = useLibrary((s) => s.minLength);
  const [recs, setRecs] = useState<Recommendation[] | null>(null);
  const [online, setOnline] = useState<OnlineSuggestion[] | null>(() => onlineCache.get(id) ?? null);
  const [searching, setSearching] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    api
      .getRecommendations(id)
      .then((r) => alive && setRecs(r))
      .catch(() => alive && setRecs([]));
    return () => {
      alive = false;
    };
  }, [id, version, libVersion]);

  useEffect(() => setOnline(onlineCache.get(id) ?? null), [id]);

  const findOnline = async () => {
    setSearching(true);
    try {
      const r = await api.findSimilarOnline(id);
      onlineCache.set(id, r);
      setOnline(r);
      if (!r.length) usePlayer.getState().pushToast("info", "Deezer had nothing new for this playlist.");
    } catch (e) {
      toastErr(e);
    } finally {
      setSearching(false);
    }
  };

  const addOnline = async (s: OnlineSuggestion, thenFind: boolean) => {
    setBusy(`dz${s.deezerId}`);
    try {
      await api.addReferenceToPlaylist(id, toRef(s));
      const left = (onlineCache.get(id) ?? []).filter((o) => o.deezerId !== s.deezerId);
      onlineCache.set(id, left);
      setOnline(left);
      if (thenFind) useUi.getState().setView({ kind: "review", id });
      else usePlayer.getState().pushToast("info", `Added "${s.title}". Use Get missing songs to download it.`);
    } catch (e) {
      toastErr(e);
    } finally {
      setBusy(null);
    }
  };

  const shown = (recs ?? []).filter((r) => longEnough(r.track, minLength));
  if (recs === null) return null;

  return (
    <section className="mx-6 mt-8 border-t border-line pt-6">
      <div className="flex items-end gap-3 px-2 pb-3">
        <div className="min-w-0 flex-1">
          <h2 className="flex items-center gap-2 text-[17px] font-bold">
            <Sparkles size={16} className="text-accent" /> Recommended songs
          </h2>
          <p className="text-[12.5px] text-muted">Based on the songs in this playlist.</p>
        </div>
        <button
          onClick={findOnline}
          disabled={searching}
          title="Looks up similar songs on Deezer. Sends the names of this playlist's main artists, nothing else."
          className={addBtn}
        >
          {searching ? <Loader2 size={13} className="animate-spin" /> : <Globe size={13} />}
          {online ? "Search again online" : "Find more online"}
        </button>
      </div>

      {shown.length === 0 && !online?.length && (
        <p className="px-2 text-[12.5px] text-muted">
          Nothing else in your library is like these songs. Try <span className="text-fg">Find more online</span>.
        </p>
      )}

      {shown.map((r) => {
        const t = r.track;
        return (
          <div
            key={t.path}
            className="group flex h-14 items-center gap-3 rounded-md px-2 transition-colors hover:bg-hover"
            onDoubleClick={() => playTracks([t], 0)}
            title={t.path}
          >
            <div className="relative h-10 w-10 shrink-0">
              <Cover coverId={liveCoverId(t)} className="h-10 w-10" rounded="rounded" iconSize={16} />
              <button
                aria-label={`Play ${t.title}`}
                title="Play"
                onClick={() => playTracks([t], 0)}
                className="absolute inset-0 flex items-center justify-center rounded bg-black/50 opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100"
              >
                <span className="flex h-7 w-7 items-center justify-center rounded-full bg-accent text-on-accent shadow-md">
                  <Play size={13} fill="currentColor" strokeWidth={0} className="translate-x-px" />
                </span>
              </button>
            </div>
            <div className="min-w-0 flex-1">
              <div className="truncate text-[13.5px]">{t.title}</div>
              <div className="truncate text-[12.5px] text-muted">{displayArtist(t)}</div>
            </div>
            <span className="hidden w-[30%] truncate text-[12px] text-faint lg:block">{r.reason}</span>
            <span className="tabular w-12 shrink-0 text-right text-[12.5px] text-muted">{t.duration > 0 ? fmtTime(t.duration) : ""}</span>
            <button
              className={addBtn}
              disabled={busy === t.path}
              onClick={async () => {
                setBusy(t.path);
                await api.addToPlaylist(id, [t.id]).catch(toastErr);
                setBusy(null);
              }}
            >
              <Plus size={13} /> Add
            </button>
          </div>
        );
      })}

      {online && online.length > 0 && (
        <>
          <div className="px-2 pb-1 pt-4 text-[12px] font-semibold text-fg/80">Not in your library</div>
          {online.map((s) => (
            <div key={s.deezerId} className="flex h-14 items-center gap-3 rounded-md px-2 transition-colors hover:bg-hover">
              <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded bg-hover text-faint">
                <Globe size={16} />
              </div>
              <div className="min-w-0 flex-1">
                <div className="truncate text-[13.5px]">{s.title}</div>
                <div className="truncate text-[12.5px] text-muted">{s.artists.join(", ")}</div>
              </div>
              <span className="hidden w-[30%] truncate text-[12px] text-faint lg:block">{s.reason}</span>
              <span className="tabular w-12 shrink-0 text-right text-[12.5px] text-muted">{s.duration > 0 ? fmtTime(s.duration) : ""}</span>
              <button
                className={addBtn}
                disabled={busy === `dz${s.deezerId}`}
                onClick={() => addOnline(s, true)}
                title="Add it to this playlist, then search Soulseek or YouTube for it"
              >
                <Download size={13} /> Find
              </button>
              <button className={addBtn} disabled={busy === `dz${s.deezerId}`} onClick={() => addOnline(s, false)} title="Add it to this playlist as a missing song">
                <Plus size={13} /> Add
              </button>
            </div>
          ))}
        </>
      )}
    </section>
  );
}
