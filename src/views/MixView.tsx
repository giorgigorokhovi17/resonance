import { ListMusic, ListPlus, Pause, Play, Shuffle } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { EmptyState } from "../components/EmptyState";
import { PlaylistCover } from "../components/PlaylistCover";
import { TrackTable } from "../components/TrackTable";
import { playShuffled, playTracks } from "../lib/actions";
import { api } from "../lib/api";
import { fmtDuration } from "../lib/format";
import type { SmartMix, Track } from "../lib/types";
import { useLibrary } from "../store/library";
import { usePlayer } from "../store/player";
import { useUi } from "../store/ui";

const toastErr = (e: unknown) => usePlayer.getState().pushToast("error", String(e));

/** Up to four distinct cover ids from a mix's tracks, for its 2x2 collage. */
export function mixPreviewCovers(tracks: Track[]): string[] {
  const seen = new Set<string>();
  const covers: string[] = [];
  for (const t of tracks) {
    if (t.coverId && !seen.has(t.coverId)) {
      seen.add(t.coverId);
      covers.push(t.coverId);
      if (covers.length === 4) break;
    }
  }
  return covers;
}

export function MixView({ id }: { id: string }) {
  const libVersion = useLibrary((s) => s.byId);
  const currentPath = usePlayer((s) => s.status?.track?.path);
  const playing = usePlayer((s) => s.status?.state === "playing");
  const queueItems = usePlayer((s) => s.queue.items);
  const [mix, setMix] = useState<SmartMix | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let alive = true;
    api
      .smartMixes()
      .then((all) => {
        if (!alive) return;
        setMix(all.find((m) => m.id === id) ?? null);
        setLoaded(true);
      })
      .catch((e) => {
        if (!alive) return;
        setError(String(e));
        setLoaded(true);
      });
    return () => {
      alive = false;
    };
  }, [id, libVersion]);

  const tracks = mix?.tracks ?? [];
  const total = useMemo(() => tracks.reduce((s, t) => s + t.duration, 0), [tracks]);
  const cover = useMemo(() => ({ coverId: null, previewCovers: mixPreviewCovers(tracks) }), [tracks]);

  if (error) return <EmptyState icon={<ListMusic size={28} />} title="Mix unavailable" body={error} />;
  if (!loaded) return null;
  if (!mix) return <EmptyState icon={<ListMusic size={28} />} title="Mix not found" />;

  const isPlayingThis =
    !!currentPath &&
    tracks.some((t) => t.path === currentPath) &&
    queueItems.length === tracks.length;

  const saveAsPlaylist = async () => {
    if (saving || !tracks.length) return;
    setSaving(true);
    try {
      const playlistId = await api.createPlaylist(
        mix.title,
        tracks.map((t) => t.id),
      );
      usePlayer.getState().pushToast("info", `Saved "${mix.title}" as a playlist.`);
      useUi.getState().setView({ kind: "playlist", id: playlistId });
    } catch (e) {
      toastErr(e);
    } finally {
      setSaving(false);
    }
  };

  const header = (
    <div>
      <div className="flex items-end gap-7 px-8 pb-6 pt-10">
        <div className="h-[212px] w-[212px] shrink-0 rounded-lg shadow-[0_16px_48px_-12px_rgba(0,0,0,0.6)]">
          <PlaylistCover playlist={cover} className="h-full w-full" iconSize={72} size="full" />
        </div>
        <div className="min-w-0 pb-1">
          <div className="text-[12px] font-semibold text-fg/80">Made for you</div>
          <h1 className="mt-1 truncate font-display text-[clamp(32px,4.4vw,64px)] font-extrabold leading-[1.05] tracking-tight">
            {mix.title}
          </h1>
          <p className="mt-3 line-clamp-2 max-w-2xl text-[13.5px] text-fg/70">{mix.description}</p>
          <div className="mt-3 text-[13px] text-fg/75">
            <span className="tabular">{tracks.length}</span> song{tracks.length === 1 ? "" : "s"}
            {total > 0 && <span className="tabular text-fg/55"> · {fmtDuration(total)}</span>}
          </div>
        </div>
      </div>
      <div className="flex items-center gap-4 px-8 pb-5">
        <button
          aria-label={isPlayingThis && playing ? "Pause" : "Play"}
          disabled={!tracks.length}
          onClick={() => (isPlayingThis ? api.togglePlay() : playTracks(tracks, 0))}
          className="flex h-14 w-14 items-center justify-center rounded-full bg-accent text-on-accent shadow-lg transition-transform duration-150 hover:scale-105 active:scale-95 disabled:opacity-40 motion-reduce:transition-none motion-reduce:hover:scale-100 motion-reduce:active:scale-100"
        >
          {isPlayingThis && playing ? (
            <Pause size={24} fill="currentColor" strokeWidth={0} />
          ) : (
            <Play size={24} fill="currentColor" strokeWidth={0} className="translate-x-0.5" />
          )}
        </button>
        <button
          aria-label="Shuffle play"
          title="Shuffle play"
          disabled={!tracks.length}
          onClick={() => playShuffled(tracks)}
          className="rounded-full p-2 text-muted transition-colors hover:text-fg disabled:opacity-40"
        >
          <Shuffle size={26} />
        </button>
        <button
          onClick={saveAsPlaylist}
          disabled={saving || !tracks.length}
          className="inline-flex items-center gap-2 rounded-full px-4 py-2 text-[13px] font-medium text-fg ring-1 ring-line transition-colors hover:bg-hover disabled:opacity-40"
        >
          <ListPlus size={15} /> {saving ? "Saving…" : "Save as playlist"}
        </button>
      </div>
    </div>
  );

  return (
    <TrackTable
      tracks={tracks}
      columns={["index", "title", "artist", "album", "year", "quality", "duration"]}
      header={header}
      emptyText="No songs in this mix yet"
      onPlay={(sorted, i) => playTracks(sorted, i)}
    />
  );
}
