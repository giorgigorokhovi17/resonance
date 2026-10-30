import { save } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, ArrowDown, ArrowUp, AudioLines, Download, Eye, EyeOff, FileOutput, GripVertical, ListChecks, ListMusic, MoreHorizontal, Pause, Pencil, Pin, PinOff, Play, Search, Shuffle, Trash2, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { openMenu, SEP, type MenuItem } from "../components/ContextMenu";
import { Cover } from "../components/Cover";
import { confirmDialog } from "../components/Dialog";
import { EmptyState } from "../components/EmptyState";
import { LyricsPanel } from "../components/LyricsPanel";
import { HiddenNote, MinLengthSelect } from "../components/MinLengthSelect";
import { PlaylistCover } from "../components/PlaylistCover";
import { PlaylistRecommendations } from "../components/PlaylistRecommendations";
import { openPlaylistEditor } from "../components/PlaylistEditor";
import { ReorderList } from "../components/ReorderList";
import { trackMenuItems } from "../components/trackMenu";
import { api } from "../lib/api";
import { displayArtist, fmtDuration, fmtTime } from "../lib/format";
import type { Playlist, PlaylistDetail, PlaylistEntry } from "../lib/types";
import { liveCoverId, longEnough, useLibrary } from "../store/library";
import { usePlayer } from "../store/player";
import { movePlaylist, setHidden, setPinned, usePlaylists } from "../store/playlists";
import { useUi } from "../store/ui";
import { SoulseekUpgrade } from "./SoulseekUpgrade";

const toastErr = (e: unknown) => usePlayer.getState().pushToast("error", String(e));

export async function deletePlaylist(p: Playlist) {
  if (await confirmDialog(`Delete "${p.name}"?`, "The playlist is removed. Your music files are not touched.")) {
    await api.deletePlaylist(p.id).catch(toastErr);
    const v = useUi.getState().view;
    if (v.kind === "playlist" && v.id === p.id) useUi.getState().setView({ kind: "playlists" }, true);
  }
}

export async function exportPlaylist(p: Playlist) {
  const path = await save({
    title: "Export playlist",
    defaultPath: `${p.name.replace(/[\\/:*?"<>|]/g, "_")}.m3u8`,
    filters: [
      { name: "M3U8 playlist", extensions: ["m3u8", "m3u"] },
      { name: "XSPF playlist", extensions: ["xspf"] },
      { name: "CSV with ISRC (spreadsheets)", extensions: ["csv"] },
    ],
  });
  if (!path) return;
  try {
    const n = await api.exportPlaylist(p.id, path);
    usePlayer.getState().pushToast("info", `Exported ${n} songs to ${path}`);
  } catch (e) {
    toastErr(e);
  }
}

export async function exportUnmatchedReport(p: Playlist) {
  const path = await save({
    title: "Export unmatched report",
    defaultPath: `${p.name.replace(/[\\/:*?"<>|]/g, "_")} - unmatched.csv`,
    filters: [{ name: "CSV", extensions: ["csv"] }],
  });
  if (!path) return;
  try {
    const n = await api.exportUnmatchedReport(p.id, path);
    usePlayer.getState().pushToast("info", n ? `${n} unmatched song${n === 1 ? "" : "s"} listed in ${path}` : "Every imported song is matched.");
  } catch (e) {
    toastErr(e);
  }
}

export function playlistItemMenu(p: Playlist): MenuItem[] {
  const all = usePlaylists.getState().playlists;
  const at = all.findIndex((x) => x.id === p.id);
  // Moving up or down stays among the pinned (or unpinned) ones; pinning changes groups. Sidebar
  // playlists move past each other, not past hidden ones they'd seem not to move over.
  const peers = all.filter((x) => x.pinned === p.pinned && x.hidden === p.hidden);
  const place = peers.findIndex((x) => x.id === p.id);
  const up = peers[place - 1];
  const down = peers[place + 1];
  return [
    { label: "Play", icon: <Play size={14} />, onClick: () => playPlaylist(p.id, 0, false) },
    { label: "Edit details…", icon: <Pencil size={14} />, onClick: () => openPlaylistEditor(p) },
    p.pinned
      ? { label: "Unpin", icon: <PinOff size={14} />, onClick: () => setPinned(p, false) }
      : { label: "Pin to top", icon: <Pin size={14} />, onClick: () => setPinned(p, true) },
    ...(up ? [{ label: "Move up", icon: <ArrowUp size={14} />, onClick: () => movePlaylist(at, all.indexOf(up)) }] : []),
    ...(down ? [{ label: "Move down", icon: <ArrowDown size={14} />, onClick: () => movePlaylist(at, all.indexOf(down)) }] : []),
    p.hidden
      ? { label: "Show in sidebar", icon: <Eye size={14} />, onClick: () => setHidden(p, false) }
      : { label: "Hide from sidebar", icon: <EyeOff size={14} />, onClick: () => setHidden(p, true) },
    { label: "Export playlist…", icon: <FileOutput size={14} />, onClick: () => exportPlaylist(p) },
    ...(p.source
      ? [
          { label: "Review matches…", icon: <ListChecks size={14} />, onClick: () => useUi.getState().setView({ kind: "review", id: p.id }) },
          { label: "Export unmatched report…", icon: <FileOutput size={14} />, onClick: () => exportUnmatchedReport(p) },
        ]
      : []),
    SEP,
    { label: "Delete", icon: <Trash2 size={14} />, danger: true, onClick: () => deletePlaylist(p) },
  ];
}

/** `start` indexes the playlist's full entry list; short tracks hidden by the library filter are skipped. */
export async function playPlaylist(id: number, start: number, shuffle: boolean) {
  try {
    const d = await api.getPlaylist(id);
    const paths = d.entries.filter((e) => !e.missing && longEnough(e.track)).map((e) => e.track.path);
    if (!paths.length) return;
    await api.setShuffle(shuffle);
    const startPath = d.entries[start]?.track.path;
    const idx = shuffle ? Math.floor(Math.random() * paths.length) : Math.max(0, paths.indexOf(startPath ?? ""));
    await api.playPaths(paths, idx);
  } catch (e) {
    toastErr(e);
  }
}

export function PlaylistView({ id }: { id: number }) {
  const version = usePlaylists((s) => s.version);
  const meta = usePlaylists((s) => s.playlists.find((p) => p.id === id));
  const libVersion = useLibrary((s) => s.byId);
  const minLength = useLibrary((s) => s.minLength);
  const [detail, setDetail] = useState<PlaylistDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [toReview, setToReview] = useState(0);
  /** Imported songs with nothing like them in the library (can be found on Soulseek). */
  const [notOwned, setNotOwned] = useState(0);
  const [scanning, setScanning] = useState(false);
  const currentPath = usePlayer((s) => s.status?.track?.path);
  const playing = usePlayer((s) => s.status?.state === "playing");
  const queuePaths = usePlayer((s) => s.queue.items);
  const split = usePlayer((s) => s.settings?.appearance.playlistLayout === "split");

  useEffect(() => {
    let alive = true;
    api
      .getPlaylist(id)
      .then((d) => alive && setDetail(d))
      .catch((e) => alive && setError(String(e)));
    return () => {
      alive = false;
    };
  }, [id, version, libVersion]);

  // Imported playlists (or ones with songs added from online recommendations): how many entries
  // wait for a decision (the matcher runs in the backend).
  const imported = !!meta?.source || !!detail?.entries.some((e) => e.refKey);
  useEffect(() => {
    let alive = true;
    if (!imported) {
      setToReview(0);
      setNotOwned(0);
      return;
    }
    api
      .getPlaylistReview(id)
      .then((r) => {
        if (!alive) return;
        setToReview(r.pending.length);
        setNotOwned(r.missing.length);
      })
      .catch(() => alive && setToReview(0));
    return () => {
      alive = false;
    };
  }, [id, imported, version, libVersion]);

  const persist = useCallback(
    (entries: PlaylistEntry[]) => {
      if (!detail) return;
      setDetail({ ...detail, entries });
      api.setPlaylistPaths(id, entries.map((e) => e.track.path)).catch(toastErr);
    },
    [detail, id],
  );

  // Visible entries (missing files stay visible so they can be cleaned up) and their index in the full list.
  const slots = useMemo(
    () =>
      (detail?.entries ?? []).flatMap((e, i) => (e.missing || longEnough(e.track, minLength) ? [i] : [])),
    [detail, minLength],
  );

  const onMove = useCallback(
    (from: number, to: number) => {
      if (!detail) return;
      // Reorder the visible entries, then put them back into the visible slots; hidden ones keep their place.
      const visible = slots.map((i) => detail.entries[i]);
      const [e] = visible.splice(from, 1);
      visible.splice(to, 0, e);
      const entries = [...detail.entries];
      slots.forEach((slot, k) => (entries[slot] = visible[k]));
      persist(entries);
    },
    [detail, slots, persist],
  );

  if (error) return <EmptyState icon={<ListMusic size={28} />} title="Playlist not found" body={error} />;
  if (!detail) return null;
  const playlist = meta ?? detail.playlist;
  const shown = slots.map((i) => detail.entries[i]);
  const hidden = detail.entries.length - shown.length;
  const playable = shown.filter((e) => !e.missing);
  const total = playable.reduce((s, e) => s + e.track.duration, 0);
  const missing = shown.length - playable.length;
  const fromYoutube = detail.entries.filter((e) => e.fromYoutube);
  // "This playlist is playing" = the current song belongs to it and the queue matches it.
  const isPlayingThis =
    !!currentPath &&
    playable.some((e) => e.track.path === currentPath) &&
    queuePaths.length === playable.length;

  // The page's parts; the Visual setting decides how they're laid out.
  const cover = (box: string) => (
    <button
      onClick={() => openPlaylistEditor(playlist)}
      className={`group relative shrink-0 rounded-lg shadow-[0_16px_48px_-12px_rgba(0,0,0,0.6)] ${box}`}
      aria-label="Edit details"
    >
      <PlaylistCover playlist={playlist} className="h-full w-full" iconSize={72} size="full" motion="page" />
      <span className="absolute inset-0 flex flex-col items-center justify-center gap-2 rounded-lg bg-black/55 text-[13px] font-semibold text-white opacity-0 transition-opacity duration-200 group-hover:opacity-100">
        <Pencil size={28} /> Edit details
      </span>
    </button>
  );

  const details = (
    <div className="min-w-0 pb-1">
      <div className="text-[12px] font-semibold text-fg/80">Playlist</div>
      <h1
        onClick={() => openPlaylistEditor(playlist)}
        className={`mt-1 cursor-pointer font-display font-extrabold leading-[1.05] tracking-tight ${
          split ? "line-clamp-2 text-[clamp(24px,2.4vw,34px)]" : "truncate text-[clamp(32px,4.4vw,64px)]"
        }`}
      >
        {playlist.name}
      </h1>
      {playlist.description && <p className="mt-3 line-clamp-2 max-w-2xl text-[13.5px] text-fg/70">{playlist.description}</p>}
      <div className="mt-3 text-[13px] text-fg/75">
        {shown.length} song{shown.length === 1 ? "" : "s"}
        {total > 0 && <span className="text-fg/55"> · {fmtDuration(total)}</span>}
        <HiddenNote count={hidden} />
        {missing > 0 && (
          <span className="ml-2 inline-flex items-center gap-1 text-warn">
            <AlertTriangle size={13} /> {missing} missing
          </span>
        )}
      </div>
      <div className="mt-3 flex flex-wrap gap-2">
      {notOwned + toReview > 0 && (
        <button
          onClick={() => useUi.getState().setView({ kind: "review", id })}
          className="accent-gradient text-on-accent inline-flex items-center gap-1.5 rounded-full px-3.5 py-1 text-[12.5px] font-semibold shadow-md transition-transform hover:scale-[1.03]"
          title="Search Soulseek for the songs you don't have, then pick what to download"
        >
          <Download size={14} /> Get {notOwned + toReview} missing song{notOwned + toReview === 1 ? "" : "s"}
        </button>
      )}
      {toReview > 0 && (
        <button
          onClick={() => useUi.getState().setView({ kind: "review", id })}
          className="inline-flex items-center gap-1.5 rounded-full bg-accent/15 px-3 py-1 text-[12.5px] font-medium text-accent ring-1 ring-accent/30 transition-colors hover:bg-accent/25"
        >
          <ListChecks size={14} /> {toReview} song{toReview === 1 ? "" : "s"} need{toReview === 1 ? "s" : ""} a decision · Review
        </button>
      )}
      {fromYoutube.length > 0 && (
        <button
          onClick={() => setScanning(true)}
          className="inline-flex items-center gap-1.5 rounded-full px-3 py-1 text-[12.5px] font-medium ring-1 ring-line transition-colors hover:bg-hover"
          title="Look on Soulseek for better copies of the songs that came from YouTube"
        >
          <Search size={14} /> Scan on Soulseek
        </button>
      )}
      </div>
    </div>
  );

  const actions = (
    <div className={`flex items-center gap-4 ${split ? "" : "px-8 pb-5"}`}>
      <button
        aria-label={isPlayingThis && playing ? "Pause" : "Play"}
        disabled={!playable.length}
        onClick={() => (isPlayingThis ? api.togglePlay() : playPlaylist(id, 0, false))}
        className="flex h-14 w-14 items-center justify-center rounded-full bg-accent text-on-accent shadow-lg transition-transform duration-150 hover:scale-105 active:scale-95 disabled:opacity-40"
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
        disabled={!playable.length}
        onClick={() => playPlaylist(id, 0, true)}
        className="rounded-full p-2 text-muted transition-colors hover:text-fg disabled:opacity-40"
      >
        <Shuffle size={26} />
      </button>
      <button
        aria-label="More options"
        onClick={(e) => openMenu(e, playlistItemMenu(playlist))}
        className="rounded-full p-2 text-muted transition-colors hover:text-fg"
      >
        <MoreHorizontal size={26} />
      </button>
      <div className="ml-auto">
        <MinLengthSelect />
      </div>
    </div>
  );

  const header = (
    <div>
      <div className="flex items-end gap-7 px-8 pb-6 pt-10">
        {cover("h-[212px] w-[212px]")}
        {details}
      </div>
      {actions}
    </div>
  );

  // With the split layout, the details sit above the songs; the cover and lyrics are on the left.
  const splitHeader = (
    <div className="space-y-5 px-8 pb-5 pt-8">
      {details}
      {actions}
    </div>
  );

  const emptyNote = (
    <p className="px-8 text-[13px] text-muted">
      {hidden > 0 ? (
        "Every song in this playlist is shorter than the minimum length."
      ) : (
        <>
          This playlist is empty. Right-click any song and choose <span className="text-fg">Add to playlist</span>.
        </>
      )}
    </p>
  );

  const list = shown.length > 0 && (
    <ReorderList<PlaylistEntry>
      items={shown}
      getKey={(e) => e.track.path + "#" + slots[shown.indexOf(e)]}
      rowHeight={56}
      onMove={onMove}
      header={split ? splitHeader : header}
      footer={<PlaylistRecommendations id={id} />}
      renderRow={(entry, index) => {
        const t = entry.track;
        const isCur = t.path === currentPath;
        const at = slots[index];
        const remove = () => persist(detail.entries.filter((_, i) => i !== at));
        return (
          <div
            className={`group mx-6 flex h-full items-center gap-3 rounded-md px-2 transition-colors duration-150 hover:bg-hover ${entry.missing ? "opacity-50" : ""}`}
            onDoubleClick={() => !entry.missing && playPlaylist(id, at, false)}
            onContextMenu={(e) =>
              openMenu(e, [
                ...(entry.missing
                  ? [
                      entry.refKey
                        ? { label: "Find on Soulseek…", icon: <Download size={14} />, onClick: () => useUi.getState().setView({ kind: "review", id }) }
                        : { label: "File not found", disabled: true, icon: <AlertTriangle size={14} /> },
                    ]
                  : trackMenuItems([t], { playAll: () => playPlaylist(id, at, false) })),
                SEP,
                { label: "Remove from playlist", icon: <X size={14} />, danger: true, onClick: remove },
              ])
            }
            title={entry.missing ? (entry.refKey ? "Not in your library yet" : `Missing: ${t.path}`) : t.path}
          >
            <GripVertical size={14} className="shrink-0 cursor-grab text-faint opacity-0 transition-opacity group-hover:opacity-100" />
            <span className="tabular w-6 shrink-0 text-right text-[12.5px] text-muted">{index + 1}</span>
            <div className="relative h-10 w-10 shrink-0">
              <Cover coverId={liveCoverId(t)} className="h-10 w-10" rounded="rounded" iconSize={16} />
              {!entry.missing && (
                // One click plays the song; it fades in over the cover while the pointer is on the row.
                <button
                  aria-label={isCur && playing ? `Pause ${t.title}` : `Play ${t.title}`}
                  title={isCur && playing ? "Pause" : "Play"}
                  onClick={(e) => {
                    e.stopPropagation();
                    if (isCur) api.togglePlay();
                    else playPlaylist(id, at, false);
                  }}
                  className={`absolute inset-0 flex items-center justify-center rounded bg-black/50 transition-opacity duration-200 focus-visible:opacity-100 ${
                    isCur ? "opacity-100" : "opacity-0 group-hover:opacity-100"
                  }`}
                >
                  <span className="flex h-7 w-7 scale-75 items-center justify-center rounded-full bg-accent text-on-accent shadow-md transition-transform duration-200 ease-out group-hover:scale-100">
                    {isCur && playing ? (
                      <>
                        <AudioLines size={14} className="animate-pulse group-hover:hidden" />
                        <Pause size={13} fill="currentColor" strokeWidth={0} className="hidden group-hover:block" />
                      </>
                    ) : (
                      <Play size={13} fill="currentColor" strokeWidth={0} className="translate-x-px" />
                    )}
                  </span>
                </button>
              )}
            </div>
            <div className="min-w-0 flex-1">
              <div className={`truncate text-[13.5px] ${isCur ? "font-medium text-accent" : ""}`}>
                {entry.missing && <AlertTriangle size={12} className="mr-1 inline text-warn" />}
                {t.title}
              </div>
              <div className="truncate text-[12.5px] text-muted">{entry.missing && !entry.refKey ? t.path : displayArtist(t)}</div>
            </div>
            <span className="hidden w-[30%] truncate text-[12.5px] text-muted lg:block">{entry.missing && !entry.refKey ? "" : t.album}</span>
            <span className="tabular w-12 shrink-0 text-right text-[12.5px] text-muted">{t.duration > 0 ? fmtTime(t.duration) : ""}</span>
            <button
              aria-label="Remove from playlist"
              onClick={remove}
              className="shrink-0 rounded p-1 text-faint opacity-0 transition-opacity hover:bg-hover hover:text-fg group-hover:opacity-100"
            >
              <X size={14} />
            </button>
          </div>
        );
      }}
    />
  );

  const scan = scanning && <SoulseekUpgrade playlistId={id} entries={fromYoutube} onClose={() => setScanning(false)} />;

  if (split) {
    // The cover with the playing song's lyrics below it on the left; details and songs on the right.
    return (
      <div className="flex h-full gap-2 pl-8">
        <aside className="flex w-[clamp(240px,27vw,340px)] shrink-0 flex-col gap-4 overflow-y-auto pb-5 pt-8">
          {cover("aspect-square w-[min(100%,40vh)]")}
          <LyricsPanel className="min-h-[200px] flex-1" />
        </aside>
        <div className="min-w-0 flex-1">
          {list || (
            <div className="h-full overflow-y-auto">
              {splitHeader}
              {emptyNote}
            </div>
          )}
        </div>
        {scan}
      </div>
    );
  }

  return (
    <>
      {list || (
        <div className="h-full overflow-y-auto">
          {header}
          {emptyNote}
        </div>
      )}
      {scan}
    </>
  );
}
