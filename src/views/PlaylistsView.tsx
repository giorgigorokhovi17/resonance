import { open } from "@tauri-apps/plugin-dialog";
import { EyeOff, FileInput, Link2, Play, Plus } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { openMenu } from "../components/ContextMenu";
import { SecondaryButton } from "../components/EmptyState";
import { PlaylistCover } from "../components/PlaylistCover";
import { openPlaylistEditor } from "../components/PlaylistEditor";
import { openLinkImport } from "../components/LinkImportDialog";
import { ViewHeader } from "../components/ViewHeader";
import { playTracks } from "../lib/actions";
import { api } from "../lib/api";
import type { Playlist, SmartMix } from "../lib/types";
import { useLibrary } from "../store/library";
import { usePlayer } from "../store/player";
import { usePlaylists } from "../store/playlists";
import { useUi } from "../store/ui";
import { mixPreviewCovers } from "./MixView";
import { playlistItemMenu, playPlaylist } from "./PlaylistView";

export async function importPlaylist() {
  const picked = await open({
    multiple: true,
    title: "Import playlist",
    filters: [
      { name: "Playlists (M3U, PLS, XSPF, WPL)", extensions: ["m3u", "m3u8", "pls", "xspf", "wpl"] },
      { name: "Undertone CSV export", extensions: ["csv"] },
    ],
  });
  if (!picked) return;
  for (const p of Array.isArray(picked) ? picked : [picked]) {
    try {
      const r = await api.importPlaylist(p);
      const notes = [
        r.missing ? `${r.missing} file(s) not found` : "",
        r.skipped ? `${r.skipped} stream(s) or web link(s) left out` : "",
        r.review ? `${r.review} need${r.review === 1 ? "s" : ""} a decision (Export unmatched report)` : "",
      ].filter(Boolean);
      usePlayer
        .getState()
        .pushToast(
          r.missing ? "error" : "info",
          `${r.updated ? "Updated" : "Imported"} "${r.name}": ${r.total} songs${notes.length ? `, ${notes.join(", ")}` : ""}.`,
        );
      useUi.getState().setView({ kind: "playlist", id: r.id });
    } catch (e) {
      usePlayer.getState().pushToast("error", String(e));
    }
  }
}

function PlaylistCard({ p }: { p: Playlist }) {
  const setView = useUi((s) => s.setView);
  return (
    <div
      data-motion-hover
      onClick={() => setView({ kind: "playlist", id: p.id })}
      onContextMenu={(e) => openMenu(e, playlistItemMenu(p))}
      className="group min-w-0 cursor-pointer rounded-lg p-2.5 transition-colors duration-200 hover:bg-hover"
    >
      <div className="relative aspect-square w-full">
        <PlaylistCover playlist={p} motion="tile" className="h-full w-full shadow-[0_8px_24px_-8px_rgba(0,0,0,0.5)]" iconSize={48} />
        <button
          aria-label={`Play ${p.name}`}
          onClick={(e) => {
            e.stopPropagation();
            playPlaylist(p.id, 0, false);
          }}
          className="absolute bottom-2 right-2 flex h-11 w-11 translate-y-1.5 items-center justify-center rounded-full bg-accent text-on-accent opacity-0 shadow-lg transition-all duration-200 hover:scale-105 group-hover:translate-y-0 group-hover:opacity-100"
        >
          <Play size={18} fill="currentColor" strokeWidth={0} className="translate-x-px" />
        </button>
      </div>
      <div className="mt-2.5 flex items-center gap-1.5 text-[13.5px] font-medium">
        <span className="min-w-0 truncate">{p.name}</span>
        {p.hidden && (
          <span title="Hidden from the sidebar" className="shrink-0 text-faint">
            <EyeOff size={13} aria-label="Hidden from the sidebar" />
          </span>
        )}
      </div>
      <div className="truncate text-[12.5px] text-muted">
        {p.description || `${p.trackCount} song${p.trackCount === 1 ? "" : "s"}`}
      </div>
    </div>
  );
}

const MIN_MIX_TRACKS = 5;

function MixCard({ mix }: { mix: SmartMix }) {
  const setView = useUi((s) => s.setView);
  const cover = useMemo(() => ({ coverId: null, previewCovers: mixPreviewCovers(mix.tracks) }), [mix.tracks]);
  return (
    <div
      onClick={() => setView({ kind: "mix", id: mix.id })}
      className="group min-w-0 cursor-pointer rounded-lg p-2.5 transition-colors duration-200 hover:bg-hover motion-reduce:transition-none"
    >
      <div className="relative aspect-square w-full">
        <PlaylistCover playlist={cover} className="h-full w-full shadow-[0_8px_24px_-8px_rgba(0,0,0,0.5)]" iconSize={48} />
        <button
          aria-label={`Play ${mix.title}`}
          onClick={(e) => {
            e.stopPropagation();
            playTracks(mix.tracks, 0);
          }}
          className="absolute bottom-2 right-2 flex h-11 w-11 translate-y-1.5 items-center justify-center rounded-full bg-accent text-on-accent opacity-0 shadow-lg transition-all duration-200 hover:scale-105 group-hover:translate-y-0 group-hover:opacity-100 motion-reduce:translate-y-0 motion-reduce:transition-none motion-reduce:hover:scale-100"
        >
          <Play size={18} fill="currentColor" strokeWidth={0} className="translate-x-px" />
        </button>
      </div>
      <div className="mt-2.5 truncate text-[13.5px] font-medium">{mix.title}</div>
      <div className="truncate text-[12.5px] text-muted">
        <span className="tabular">{mix.tracks.length}</span> songs
      </div>
    </div>
  );
}

export function PlaylistsView() {
  const playlists = usePlaylists((s) => s.playlists);
  const libVersion = useLibrary((s) => s.byId);
  const [mixes, setMixes] = useState<SmartMix[]>([]);

  useEffect(() => {
    let alive = true;
    api
      .smartMixes()
      .then((all) => alive && setMixes(all.filter((m) => m.tracks.length >= MIN_MIX_TRACKS)))
      .catch(() => alive && setMixes([]));
    return () => {
      alive = false;
    };
  }, [libVersion]);

  return (
    <div className="h-full overflow-y-auto">
      <ViewHeader
        title="Playlists"
        subtitle={`${playlists.length} playlist${playlists.length === 1 ? "" : "s"}`}
        actions={
          <>
            <SecondaryButton onClick={openLinkImport} title="Spotify, YouTube or Apple Music link">
              <Link2 size={15} /> Import from link
            </SecondaryButton>
            <SecondaryButton onClick={importPlaylist} title="M3U, PLS, XSPF, WPL or CSV file">
              <FileInput size={15} /> Import file
            </SecondaryButton>
          </>
        }
      />
      {mixes.length > 0 && (
        <section className="pb-6">
          <h2 className="px-8 pb-2 font-display text-[17px] font-semibold">Made for you</h2>
          <div className="grid grid-cols-[repeat(auto-fill,minmax(184px,1fr))] gap-1 px-6">
            {mixes.map((mix) => (
              <MixCard key={mix.id} mix={mix} />
            ))}
          </div>
        </section>
      )}
      {mixes.length > 0 && <h2 className="px-8 pb-2 font-display text-[17px] font-semibold">Your playlists</h2>}
      <div className="grid grid-cols-[repeat(auto-fill,minmax(184px,1fr))] gap-1 px-6 pb-10">
        <div
          onClick={() => openPlaylistEditor(null)}
          className="group cursor-pointer rounded-lg p-2.5 transition-colors duration-200 hover:bg-hover"
        >
          <div className="flex aspect-square w-full items-center justify-center rounded-lg border-2 border-dashed border-line text-muted transition-colors duration-200 group-hover:border-accent group-hover:text-accent">
            <Plus size={44} strokeWidth={1.4} />
          </div>
          <div className="mt-2.5 text-[13.5px] font-medium">Create playlist</div>
          <div className="text-[12.5px] text-muted">Name, photo, description</div>
        </div>
        {playlists.map((p) => (
          <PlaylistCard key={p.id} p={p} />
        ))}
      </div>
    </div>
  );
}
