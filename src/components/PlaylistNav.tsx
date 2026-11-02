import { Pin } from "lucide-react";
import { useMemo } from "react";
import { movePlaylist, usePlaylists } from "../store/playlists";
import { useUi } from "../store/ui";
import { playlistItemMenu } from "../views/PlaylistView";
import { openMenu } from "./ContextMenu";
import { PlaylistCover } from "./PlaylistCover";
import { ReorderList } from "./ReorderList";

/**
 * The user's playlists, with thumbnails, in the sidebar. Drag one to move it; pinned ones stay on
 * top. Hidden ones are only on the Playlists page.
 */
export function PlaylistNav() {
  const all = usePlaylists((s) => s.playlists);
  const view = useUi((s) => s.view);
  const nowPlaying = useUi((s) => s.nowPlaying);
  const setView = useUi((s) => s.setView);
  const playlists = useMemo(() => all.filter((p) => !p.hidden), [all]);
  if (!playlists.length) {
    return (
      <p className="px-5 py-2 text-[12px] leading-relaxed text-faint">
        {all.length ? "Your hidden playlists are on the Playlists page." : "Your playlists show up here."}
      </p>
    );
  }
  return (
    <ReorderList
      items={playlists}
      getKey={(p) => p.id}
      rowHeight={54}
      onMove={(from, to) => movePlaylist(all.indexOf(playlists[from]), all.indexOf(playlists[to]))}
      className="px-2"
      renderRow={(p) => {
        const active = !nowPlaying && view.kind === "playlist" && view.id === p.id;
        const open = () => setView({ kind: "playlist", id: p.id });
        return (
          <div
            role="button"
            tabIndex={0}
            data-motion-hover
            onClick={open}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                open();
              }
            }}
            onContextMenu={(e) => openMenu(e, playlistItemMenu(p))}
            className={`flex h-[52px] cursor-pointer items-center gap-3 rounded-lg p-1.5 text-left transition-colors duration-200 ${
              active ? "bg-hover" : "hover:bg-hover/70"
            }`}
            title={p.name}
          >
            <PlaylistCover playlist={p} motion="tile" className="h-10 w-10 shrink-0" rounded="rounded-md" iconSize={18} />
            <span className="min-w-0 flex-1">
              <span className={`block truncate text-[13px] ${active ? "font-semibold text-accent" : "text-fg"}`}>{p.name}</span>
              <span className="flex items-center gap-1 truncate text-[11.5px] text-muted">
                {p.pinned && <Pin size={11} className="shrink-0 text-accent" aria-label="Pinned" />}
                <span className="truncate">
                  Playlist · {p.trackCount} song{p.trackCount === 1 ? "" : "s"}
                </span>
              </span>
            </span>
          </div>
        );
      }}
    />
  );
}
