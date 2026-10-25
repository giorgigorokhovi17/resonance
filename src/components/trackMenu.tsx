import { FolderOpen, Image, ImageMinus, ImageOff, ImagePlus, ListEnd, ListPlus, ListStart, Play, Trash2 } from "lucide-react";
import { addToQueue, playNext, playTracks } from "../lib/actions";
import { api } from "../lib/api";
import { importCover, importError, pickMedia } from "../lib/mediaImport";
import type { Track } from "../lib/types";
import { folderOf, useLibrary } from "../store/library";
import { usePlayer } from "../store/player";
import { useUi } from "../store/ui";
import { SEP, type MenuItem } from "./ContextMenu";
import { confirmDialog } from "./Dialog";
import { playlistSubmenu } from "./playlistMenu";

async function setArtworkHidden(tracks: Track[], hidden: boolean) {
  const ids = tracks.filter((t) => t.id > 0).map((t) => t.id);
  try {
    await api.setTracksCoverHidden(ids, hidden);
    usePlayer
      .getState()
      .pushToast("info", `${hidden ? "Removed" : "Restored"} artwork for ${ids.length === 1 ? `"${tracks[0].title}"` : `${ids.length} songs`}.`);
  } catch (e) {
    usePlayer.getState().pushToast("error", String(e));
  }
}

/**
 * Pick a picture, GIF or short video and use it as the artwork for these library songs everywhere
 * in the app. A GIF or video moves where artwork animates; everywhere else its first frame shows.
 */
async function chooseArtwork(tracks: Track[]) {
  const ids = tracks.filter((t) => t.id > 0).map((t) => t.id);
  const picked = await pickMedia(ids.length === 1 ? `Artwork for "${tracks[0].title}"` : `Artwork for ${ids.length} songs`);
  if (!picked) return;
  try {
    const art = await importCover(picked);
    await api.setTracksCover(ids, art.coverId, art.motionId);
    const still = art.motionId && (usePlayer.getState().settings?.appearance.animatedArt ?? "off") === "off";
    usePlayer
      .getState()
      .pushToast(
        "info",
        `Set artwork for ${ids.length === 1 ? `"${tracks[0].title}"` : `${ids.length} songs`}.${
          still ? " GIFs and videos show a still frame until you turn them on in Settings → Visual." : ""
        }`,
      );
  } catch (e) {
    usePlayer.getState().pushToast("error", importError(e));
  }
}

async function resetArtwork(tracks: Track[]) {
  try {
    await api.setTracksCover(tracks.map((t) => t.id), null);
  } catch (e) {
    usePlayer.getState().pushToast("error", String(e));
  }
}

/** Context-menu items for one or more tracks. */
export function trackMenuItems(tracks: Track[], opts: { playAll?: () => void; extra?: MenuItem[] } = {}): MenuItem[] {
  const one = tracks.length === 1 ? tracks[0] : null;
  const items: MenuItem[] = [
    { label: "Play", icon: <Play size={14} />, onClick: opts.playAll ?? (() => playTracks(tracks, 0)) },
    { label: "Play next", icon: <ListStart size={14} />, onClick: () => playNext(tracks) },
    { label: "Add to queue", icon: <ListEnd size={14} />, onClick: () => addToQueue(tracks) },
    { label: "Add to playlist", icon: <ListPlus size={14} />, submenu: playlistSubmenu(tracks) },
  ];
  if (one) {
    const roots = useLibrary.getState().roots;
    const dir = folderOf(one.path);
    if (roots.some((r) => dir.startsWith(r))) {
      items.push(SEP, {
        label: "Go to folder",
        icon: <FolderOpen size={14} />,
        onClick: () => useUi.getState().setView({ kind: "folders", path: dir }),
      });
    }
  }
  // Artwork visibility (library songs only; files are never modified).
  const byId = useLibrary.getState().byId;
  const lib = tracks.map((t) => byId.get(t.id)).filter((t): t is Track => !!t);
  if (lib.length) {
    const anyHidden = lib.some((t) => t.coverHidden);
    const anyShown = lib.some((t) => !t.coverHidden && t.coverId);
    items.push(SEP, {
      label: lib.length === 1 ? "Set artwork…" : `Set artwork for ${lib.length} songs…`,
      icon: <ImagePlus size={14} />,
      onClick: () => chooseArtwork(lib),
    });
    const custom = lib.filter((t) => t.customCover);
    if (custom.length) {
      items.push({ label: "Use original artwork", icon: <ImageMinus size={14} />, onClick: () => resetArtwork(custom) });
    }
    if (anyShown) {
      items.push({ label: "Remove artwork", icon: <ImageOff size={14} />, onClick: () => setArtworkHidden(lib, true) });
    }
    if (anyHidden) {
      items.push({ label: "Restore artwork", icon: <Image size={14} />, onClick: () => setArtworkHidden(lib, false) });
    }
  }
  if (opts.extra?.length) {
    items.push(SEP, ...opts.extra);
  }
  if (lib.length) {
    items.push(SEP, {
      label: lib.length === 1 ? "Delete from disk…" : `Delete ${lib.length} songs from disk…`,
      icon: <Trash2 size={14} />,
      danger: true,
      onClick: () => deleteFromDisk(lib),
    });
  }
  return items;
}

/** Asks, then sends the songs' files to the Recycle Bin (they can be put back from Settings). */
async function deleteFromDisk(tracks: Track[]) {
  const names = tracks.slice(0, 3).map((t) => `"${t.title}"`);
  const more = tracks.length > 3 ? ` and ${tracks.length - 3} more` : "";
  const one = tracks.length === 1;
  const ok = await confirmDialog(
    one ? `Delete "${tracks[0].title}" from disk?` : `Delete ${tracks.length} songs from disk?`,
    `${one ? "The file goes" : `${names.join(", ")}${more}. The files go`} to the Recycle Bin. ` +
      `You can put ${one ? "it" : "them"} back from Settings → Recently deleted. ` +
      `Playlists keep ${one ? "it" : "them"} as missing until then.`,
  );
  if (!ok) return;
  const toast = usePlayer.getState().pushToast;
  try {
    const r = await api.deleteTracksFromDisk(tracks.map((t) => t.id));
    if (r.deleted) {
      const what = one ? `"${tracks[0].title}"` : `${r.deleted} song${r.deleted === 1 ? "" : "s"}`;
      toast("info", `Moved ${what} to the Recycle Bin.`);
    }
    if (r.failed.length === 1) toast("error", `Couldn't delete "${r.failed[0].title}". ${r.failed[0].reason}`);
    else if (r.failed.length) toast("error", `Couldn't delete ${r.failed.length} songs. ${r.failed[0].reason}`);
  } catch (e) {
    toast("error", String(e));
  }
}
