import { usePlaylists } from "../store/playlists";
import { useUi } from "../store/ui";
import { playlistItemMenu } from "../views/PlaylistView";
import { openMenu } from "./ContextMenu";
import { PlaylistCover } from "./PlaylistCover";

/** The user's playlists, with thumbnails, in the sidebar. */
export function PlaylistNav() {
  const playlists = usePlaylists((s) => s.playlists);
  const view = useUi((s) => s.view);
  const nowPlaying = useUi((s) => s.nowPlaying);
  const setView = useUi((s) => s.setView);
  if (!playlists.length) {
    return <p className="px-3 py-2 text-[12px] leading-relaxed text-faint">Your playlists show up here.</p>;
  }
  return (
    <div className="flex flex-col gap-0.5">
      {playlists.map((p) => {
        const active = !nowPlaying && view.kind === "playlist" && view.id === p.id;
        return (
          <button
            key={p.id}
            onClick={() => setView({ kind: "playlist", id: p.id })}
            onContextMenu={(e) => openMenu(e, playlistItemMenu(p))}
            className={`flex items-center gap-3 rounded-lg p-1.5 text-left transition-colors duration-200 ${
              active ? "bg-hover" : "hover:bg-hover/70"
            }`}
            title={p.name}
          >
            <PlaylistCover playlist={p} className="h-10 w-10 shrink-0" rounded="rounded-md" iconSize={18} />
            <span className="min-w-0">
              <span className={`block truncate text-[13px] ${active ? "font-semibold text-accent" : "text-fg"}`}>{p.name}</span>
              <span className="block truncate text-[11.5px] text-muted">
                Playlist · {p.trackCount} song{p.trackCount === 1 ? "" : "s"}
              </span>
            </span>
          </button>
        );
      })}
    </div>
  );
}
