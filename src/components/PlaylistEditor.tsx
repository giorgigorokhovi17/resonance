import { Film, ImagePlus, Loader2, Pencil, Trash2, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { create } from "zustand";
import { api } from "../lib/api";
import { importBackground, importCover, importError, pickMedia } from "../lib/mediaImport";
import type { Backdrop as BackdropOptions, Playlist, Track } from "../lib/types";
import { usePlayer } from "../store/player";
import { useUi } from "../store/ui";
import { Backdrop, useBackdropReadability } from "./Backdrop";
import { PlaylistCover } from "./PlaylistCover";
import { Slider } from "./Slider";

interface EditorState {
  open: null | { playlist: Playlist | null; tracks: Track[]; navigate: boolean };
}
const useEditor = create<EditorState>(() => ({ open: null }));

/** Open the "Edit details" dialog. Pass `null` to create a new playlist (optionally with tracks). */
export function openPlaylistEditor(playlist: Playlist | null, tracks: Track[] = [], navigate = true) {
  useEditor.setState({ open: { playlist, tracks, navigate } });
}

const toastErr = (e: unknown) => usePlayer.getState().pushToast("error", importError(e));
const same = (a: unknown, b: unknown) => JSON.stringify(a ?? null) === JSON.stringify(b ?? null);

export function PlaylistEditorHost() {
  const open = useEditor((s) => s.open);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [coverId, setCoverId] = useState<string | null>(null);
  const [motionId, setMotionId] = useState<string | null>(null);
  const [background, setBackground] = useState<BackdropOptions | null>(null);
  const [importing, setImporting] = useState<"cover" | "background" | null>(null);
  const [busy, setBusy] = useState(false);
  const nameRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!open) return;
    setName(open.playlist?.name ?? (open.tracks.length === 1 ? open.tracks[0].title : "My playlist"));
    setDescription(open.playlist?.description ?? "");
    setCoverId(open.playlist?.coverId ?? null);
    setMotionId(open.playlist?.motionId ?? null);
    setBackground(open.playlist?.background ?? null);
    setImporting(null);
    setTimeout(() => nameRef.current?.select(), 40);
  }, [open]);

  if (!open) return null;
  const close = () => useEditor.setState({ open: null });
  const isNew = !open.playlist;

  const pickPhoto = async () => {
    const picked = await pickMedia("Choose a playlist photo, GIF or video");
    if (!picked) return;
    setImporting("cover");
    try {
      const art = await importCover(picked);
      setCoverId(art.coverId);
      setMotionId(art.motionId);
    } catch (e) {
      toastErr(e);
    } finally {
      setImporting(null);
    }
  };

  const pickBackground = async () => {
    const picked = await pickMedia("Choose a background photo, GIF or video");
    if (!picked) return;
    setImporting("background");
    try {
      const m = await importBackground(picked);
      setBackground((b) => ({ fit: "cover", dim: null, blur: 0, ...b, mediaId: m.id }));
    } catch (e) {
      toastErr(e);
    } finally {
      setImporting(null);
    }
  };

  const save = async () => {
    if (!name.trim() || busy || importing) return;
    setBusy(true);
    try {
      let id = open.playlist?.id;
      if (id === undefined) {
        id = await api.createPlaylist(name.trim(), open.tracks.filter((t) => t.id > 0).map((t) => t.id));
      }
      await api.updatePlaylist(id, name.trim(), description);
      const p = open.playlist;
      if ((p?.coverId ?? null) !== coverId || (p?.motionId ?? null) !== motionId) await api.setPlaylistCover(id, coverId, motionId);
      if (!same(p?.background, background)) await api.setPlaylistBackground(id, background);
      close();
      if (isNew && open.navigate) useUi.getState().setView({ kind: "playlist", id });
    } catch (e) {
      toastErr(e);
    } finally {
      setBusy(false);
    }
  };

  const preview = { coverId, motionId, previewCovers: open.playlist?.previewCovers ?? [] };

  return (
    <div className="fixed inset-0 z-[120] flex items-center justify-center bg-black/55 backdrop-blur-sm animate-in" onMouseDown={close}>
      <div
        role="dialog"
        aria-modal="true"
        aria-label={isNew ? "Create playlist" : "Edit details"}
        className="max-h-[calc(100vh-32px)] w-[600px] overflow-y-auto rounded-2xl bg-panel p-6 shadow-2xl ring-1 ring-line"
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
              disabled={!!importing}
              className="group relative block h-[180px] w-[180px] overflow-hidden rounded-lg shadow-xl"
              aria-label="Choose photo"
              title="A picture, GIF or short video (up to 10 s)"
            >
              <PlaylistCover playlist={preview} className="h-full w-full" iconSize={56} size="full" motion="page" />
              <span
                className={`absolute inset-0 flex flex-col items-center justify-center gap-2 bg-black/60 text-[13px] font-semibold text-white transition-opacity duration-200 ${
                  importing === "cover" ? "opacity-100" : "opacity-0 group-hover:opacity-100"
                }`}
              >
                {importing === "cover" ? <Loader2 size={28} className="animate-spin" /> : coverId ? <Pencil size={28} /> : <ImagePlus size={30} />}
                {importing === "cover" ? "Importing…" : coverId ? "Change photo" : "Choose photo"}
              </span>
            </button>
            {coverId && (
              <button
                onClick={() => {
                  setCoverId(null);
                  setMotionId(null);
                }}
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

        <BackgroundSection
          name={name}
          background={background}
          importing={importing === "background"}
          onPick={pickBackground}
          onChange={setBackground}
        />

        <div className="mt-5 flex items-center justify-between gap-4">
          <p className="text-[11px] text-faint">
            Pictures, GIFs and videos are copied into Undertone; the original file isn't needed afterwards. Videos never play sound.
          </p>
          <button
            onClick={save}
            disabled={!name.trim() || busy || !!importing}
            className="shrink-0 rounded-full bg-fg px-7 py-2.5 text-[14px] font-bold text-bg transition-transform hover:scale-[1.03] active:scale-[0.98] disabled:opacity-50"
          >
            {isNew ? "Create" : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
}

/** The playlist page's background: choose, live preview, fit, dim, blur, remove. */
function BackgroundSection({
  name,
  background,
  importing,
  onPick,
  onChange,
}: {
  name: string;
  background: BackdropOptions | null;
  importing: boolean;
  onPick: () => void;
  onChange: (b: BackdropOptions | null) => void;
}) {
  const read = useBackdropReadability(background, "elev");
  const set = (p: Partial<BackdropOptions>) => background && onChange({ ...background, ...p });
  return (
    <div className="mt-5 border-t border-line pt-4">
      <div className="mb-3 flex items-center justify-between">
        <div>
          <div className="text-[13px] font-semibold">Background</div>
          <div className="text-[11.5px] text-faint">A photo, GIF or video (up to 60 s, 1080p) behind the playlist's page.</div>
        </div>
        <div className="flex shrink-0 gap-1.5">
          <button
            onClick={onPick}
            disabled={importing}
            className="inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-[12.5px] font-medium ring-1 ring-line transition-colors hover:bg-hover disabled:opacity-50"
          >
            {importing ? <Loader2 size={14} className="animate-spin" /> : <Film size={14} />}
            {importing ? "Importing…" : background ? "Change…" : "Choose…"}
          </button>
          {background && (
            <button
              onClick={() => onChange(null)}
              aria-label="Remove background"
              title="Remove background"
              className="rounded-full p-2 text-muted ring-1 ring-line transition-colors hover:bg-hover hover:text-fg"
            >
              <Trash2 size={14} />
            </button>
          )}
        </div>
      </div>
      {background && (
        <div className="flex gap-4">
          <div className="relative h-[132px] w-[236px] shrink-0 overflow-hidden rounded-lg bg-elev ring-1 ring-line">
            <Backdrop backdrop={background} place="page" scrim="elev" />
            <div className="relative px-3 pt-6">
              <div className="text-[10px] font-semibold text-fg/80">Playlist</div>
              <div className="truncate font-display text-[20px] font-extrabold leading-tight">{name || "My playlist"}</div>
              <div className="mt-2 space-y-1">
                {["First song", "Second song"].map((t) => (
                  <div key={t} className="truncate text-[11px]">
                    {t} <span className="text-muted">· Artist</span>
                  </div>
                ))}
              </div>
            </div>
          </div>
          <div className="flex min-w-0 flex-1 flex-col justify-center gap-2">
            <div className="flex items-center justify-between gap-2 text-[12.5px]">
              <span>Fit</span>
              <div className="inline-flex rounded-lg bg-bg p-0.5 ring-1 ring-line">
                {(["cover", "contain"] as const).map((f) => (
                  <button
                    key={f}
                    onClick={() => set({ fit: f })}
                    className={`rounded-md px-2.5 py-1 text-[12px] font-medium ${background.fit === f ? "bg-panel text-fg shadow ring-1 ring-line" : "text-muted hover:text-fg"}`}
                  >
                    {f === "cover" ? "Fill" : "Whole picture"}
                  </button>
                ))}
              </div>
            </div>
            <div className="flex items-center justify-between gap-2 text-[12.5px]">
              <span>Dim</span>
              <Slider
                label="Dim"
                width="w-28"
                value={read.chosen}
                min={0}
                max={0.8}
                step={0.01}
                format={(v) => `${Math.round(v * 100)}%`}
                onChange={(v) => set({ dim: v })}
              />
            </div>
            <div className="flex items-center justify-between gap-2 text-[12.5px]">
              <span>Blur</span>
              <Slider label="Blur" width="w-28" value={background.blur} min={0} max={40} step={1} format={(v) => `${v} px`} onChange={(v) => set({ blur: v })} />
            </div>
            {read.raised && (
              <div className="text-[11px] text-faint">Dimmed to {Math.round(read.dim * 100)}% here so text stays readable.</div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
