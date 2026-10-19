import { open } from "@tauri-apps/plugin-dialog";
import { ImagePlus, Pencil, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { create } from "zustand";
import { api } from "../lib/api";
import type { Playlist, Track } from "../lib/types";
import { usePlayer } from "../store/player";
import { useUi } from "../store/ui";
import { PlaylistCover } from "./PlaylistCover";

interface EditorState {
  open: null | { playlist: Playlist | null; tracks: Track[]; navigate: boolean };
}
const useEditor = create<EditorState>(() => ({ open: null }));

/** Open the "Edit details" dialog. Pass `null` to create a new playlist (optionally with tracks). */
export function openPlaylistEditor(playlist: Playlist | null, tracks: Track[] = [], navigate = true) {
  useEditor.setState({ open: { playlist, tracks, navigate } });
}

const toastErr = (e: unknown) => usePlayer.getState().pushToast("error", String(e));

export function PlaylistEditorHost() {
  const open = useEditor((s) => s.open);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [coverId, setCoverId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const nameRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!open) return;
    setName(open.playlist?.name ?? (open.tracks.length === 1 ? open.tracks[0].title : "My playlist"));
    setDescription(open.playlist?.description ?? "");
    setCoverId(open.playlist?.coverId ?? null);
    setTimeout(() => nameRef.current?.select(), 40);
  }, [open]);

  if (!open) return null;
  const close = () => useEditor.setState({ open: null });
  const isNew = !open.playlist;

  const pickPhoto = async () => {
    const picked = await open_image();
    if (!picked) return;
    try {
      setCoverId(await api.importCoverImage(picked));
    } catch (e) {
      toastErr(e);
    }
  };

  const save = async () => {
    if (!name.trim() || busy) return;
    setBusy(true);
    try {
      let id = open.playlist?.id;
      if (id === undefined) {
        id = await api.createPlaylist(name.trim(), open.tracks.filter((t) => t.id > 0).map((t) => t.id));
      }
      await api.updatePlaylist(id, name.trim(), description);
      if ((open.playlist?.coverId ?? null) !== coverId) await api.setPlaylistCover(id, coverId);
      close();
      if (isNew && open.navigate) useUi.getState().setView({ kind: "playlist", id });
    } catch (e) {
      toastErr(e);
    } finally {
      setBusy(false);
    }
  };

  const preview = { coverId, previewCovers: open.playlist?.previewCovers ?? [] };

  return (
    <div className="fixed inset-0 z-[120] flex items-center justify-center bg-black/55 backdrop-blur-sm animate-in" onMouseDown={close}>
      <div
        role="dialog"
        aria-modal="true"
        aria-label={isNew ? "Create playlist" : "Edit details"}
        className="w-[560px] rounded-2xl bg-panel p-6 shadow-2xl ring-1 ring-line"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") close();
          if (e.key === "Enter" && (e.target as HTMLElement).tagName !== "TEXTAREA") save();
          e.stopPropagation();
        }}
      >
        <div className="mb-5 flex items-center justify-between">
          <h2 className="text-[18px] font-bold">{isNew ? "Create playlist" : "Edit details"}</h2>
          <button aria-label="Close" onClick={close} className="rounded-full p-1.5 text-muted transition-colors hover:bg-hover hover:text-fg">
            <X size={18} />
          </button>
        </div>
        <div className="flex gap-5">
          <div className="shrink-0">
            <button
              onClick={pickPhoto}
              className="group relative block h-[180px] w-[180px] overflow-hidden rounded-lg shadow-xl"
              aria-label="Choose photo"
            >
              <PlaylistCover playlist={preview} className="h-full w-full" iconSize={56} size="full" />
              <span className="absolute inset-0 flex flex-col items-center justify-center gap-2 bg-black/60 text-[13px] font-semibold text-white opacity-0 transition-opacity duration-200 group-hover:opacity-100">
                {coverId ? <Pencil size={28} /> : <ImagePlus size={30} />}
                {coverId ? "Change photo" : "Choose photo"}
              </span>
            </button>
            {coverId && (
              <button
                onClick={() => setCoverId(null)}
                className="mt-2 w-full text-center text-[12px] text-muted transition-colors hover:text-fg hover:underline"
              >
                Remove photo
              </button>
            )}
          </div>
          <div className="flex min-w-0 flex-1 flex-col gap-3">
            <label className="block">
              <span className="mb-1 block text-[11.5px] font-semibold text-muted">Name</span>
              <input
                ref={nameRef}
                value={name}
                maxLength={120}
                onChange={(e) => setName(e.target.value)}
                className="h-10 w-full rounded-md bg-bg/70 px-3 text-[14px] outline-none ring-1 ring-line transition-shadow focus:ring-2 focus:ring-accent"
              />
            </label>
            <label className="flex flex-1 flex-col">
              <span className="mb-1 block text-[11.5px] font-semibold text-muted">Description</span>
              <textarea
                value={description}
                maxLength={500}
                onChange={(e) => setDescription(e.target.value)}
                placeholder="Add an optional description"
                className="min-h-[96px] w-full flex-1 resize-none rounded-md bg-bg/70 px-3 py-2 text-[13px] outline-none ring-1 ring-line transition-shadow placeholder:text-faint focus:ring-2 focus:ring-accent"
              />
            </label>
          </div>
        </div>
        <div className="mt-5 flex items-center justify-between">
          <p className="text-[11px] text-faint">Photos are copied into Resonance; the original file isn't needed afterwards.</p>
          <button
            onClick={save}
            disabled={!name.trim() || busy}
            className="rounded-full bg-fg px-7 py-2.5 text-[14px] font-bold text-bg transition-transform hover:scale-[1.03] active:scale-[0.98] disabled:opacity-50"
          >
            {isNew ? "Create" : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
}

async function open_image(): Promise<string | null> {
  const picked = await open({
    multiple: false,
    title: "Choose a playlist photo",
    filters: [{ name: "Images", extensions: ["jpg", "jpeg", "png", "webp", "gif", "bmp"] }],
  });
  return picked && !Array.isArray(picked) ? picked : null;
}
