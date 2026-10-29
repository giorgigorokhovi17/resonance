import { FileInput, Link2, Play, Plus } from "lucide-react";
import { openMenu } from "../components/ContextMenu";
import { SecondaryButton } from "../components/EmptyState";
import { PlaylistCover } from "../components/PlaylistCover";
import { openPlaylistEditor } from "../components/PlaylistEditor";
import { openLinkImport } from "../components/LinkImportDialog";
import { ViewHeader } from "../components/ViewHeader";
import { api } from "../lib/api";
import { pickFiles } from "../lib/files";
import type { Playlist } from "../lib/types";
import { usePlayer } from "../store/player";
import { usePlaylists } from "../store/playlists";
import { useUi } from "../store/ui";
import { playlistItemMenu, playPlaylist } from "./PlaylistView";

export async function importPlaylist() {
  const picked = await pickFiles({
    multiple: true,
    title: "Import playlist",
    filters: [
      { name: "Playlists (M3U, PLS, XSPF, WPL)", extensions: ["m3u", "m3u8", "pls", "xspf", "wpl"] },
      { name: "Undertone CSV export", extensions: ["csv"] },
    ],
  });
  for (const p of picked) {
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
      <div className="mt-2.5 truncate text-[13.5px] font-medium">{p.name}</div>
      <div className="truncate text-[12.5px] text-muted">
        {p.description || `${p.trackCount} song${p.trackCount === 1 ? "" : "s"}`}
      </div>
    </div>
  );
}

export function PlaylistsView() {
  const playlists = usePlaylists((s) => s.playlists);
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
