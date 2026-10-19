import { ListMusic, Plus } from "lucide-react";
import { api } from "../lib/api";
import type { Track } from "../lib/types";
import { usePlayer } from "../store/player";
import { usePlaylists } from "../store/playlists";
import type { MenuItem } from "./ContextMenu";
import { openPlaylistEditor } from "./PlaylistEditor";

const libIds = (tracks: Track[]) => tracks.filter((t) => t.id > 0).map((t) => t.id);

/** "Add to playlist" submenu: new playlist + existing ones. */
export function playlistSubmenu(tracks: Track[]): MenuItem[] {
  const pls = usePlaylists.getState().playlists;
  const items: MenuItem[] = [
    { label: "New playlist…", icon: <Plus size={14} />, onClick: () => openPlaylistEditor(null, tracks, false) },
  ];
  if (pls.length) items.push({ label: "", separator: true });
  for (const p of pls.slice(0, 40)) {
    items.push({
      label: p.name,
      icon: <ListMusic size={14} />,
      onClick: async () => {
        try {
          const n = await api.addToPlaylist(p.id, libIds(tracks));
          usePlayer.getState().pushToast("info", `Added ${n} song${n === 1 ? "" : "s"} to "${p.name}".`);
        } catch (e) {
          usePlayer.getState().pushToast("error", String(e));
        }
      },
    });
  }
  return items;
}
