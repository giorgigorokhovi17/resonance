import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { api } from "../lib/api";
import type { ScanProgress, ScanSummary, Track } from "../lib/types";
import { usePlayer } from "./player";

export interface FolderNode {
  path: string;
  name: string;
  children: string[];
  tracks: Track[];
  /** Total tracks below this node (recursive). */
  total: number;
}

interface LibraryStore {
  loaded: boolean;
  /** Every scanned track, including ones hidden by the short-track filter. */
  allTracks: Track[];
  /** Tracks shown in the library views (Songs, Tracks, Folders, Search): `allTracks` minus short ones. */
  tracks: Track[];
  /** Hide tracks shorter than this many seconds (and ones of unknown length); 0 shows everything. */
  minLength: MinLength;
  setMinLength: (v: MinLength) => void;
  folderRoots: string[];
  byId: Map<number, Track>;
  byPath: Map<string, Track>;
  folders: Map<string, FolderNode>;
  roots: string[];
  haystack: string[];
  scanning: boolean;
  progress: ScanProgress | null;
  load: () => Promise<void>;
}

const collator = new Intl.Collator(undefined, { sensitivity: "base", numeric: true });
export const compareText = (a: string, b: string) => collator.compare(a, b);

function sep(path: string) {
  return path.includes("\\") ? "\\" : "/";
}

function parentOf(path: string): string {
  const s = sep(path);
  const i = path.lastIndexOf(s);
  return i > 0 ? path.slice(0, i) : path;
}

export function folderOf(path: string): string {
  return parentOf(path);
}

function build(tracks: Track[], rootPaths: string[]) {
  const byId = new Map<number, Track>();
  const byPath = new Map<string, Track>();
  const haystack: string[] = new Array(tracks.length);
  tracks.forEach((t, i) => {
    byId.set(t.id, t);
    byPath.set(t.path, t);
    haystack[i] = `${t.title} ${t.artist} ${t.album} ${t.albumArtist} ${t.genre}`.toLowerCase();
  });

  // Folder tree, rooted at the library folders.
  const folders = new Map<string, FolderNode>();
  const ensure = (path: string): FolderNode => {
    let n = folders.get(path);
    if (!n) {
      const s = sep(path);
      n = { path, name: path.split(s).filter(Boolean).pop() ?? path, children: [], tracks: [], total: 0 };
      folders.set(path, n);
    }
    return n;
  };
  const roots = [...rootPaths];
  const rootSet = new Set(roots);
  const childSets = new Map<string, Set<string>>();
  roots.forEach(ensure);
  for (const t of tracks) {
    const dir = parentOf(t.path);
    const node = ensure(dir);
    node.tracks.push(t);
    node.total++;
    let child = dir;
    while (!rootSet.has(child)) {
      const parent = parentOf(child);
      if (parent === child) break;
      const p = ensure(parent);
      let cs = childSets.get(parent);
      if (!cs) childSets.set(parent, (cs = new Set()));
      cs.add(child);
      p.total++;
      child = parent;
    }
  }
  for (const [path, cs] of childSets) {
    folders.get(path)!.children = [...cs].sort((a, b) => compareText(a, b));
  }
  for (const n of folders.values()) {
    n.tracks.sort((a, b) => compareText(a.path, b.path));
  }
  return { byId, byPath, folders, roots, haystack };
}

/** Short-track filter choices, in seconds (like Poweramp's "Ignore short tracks"). */
export const MIN_LENGTHS = [0, 10, 20, 30, 60] as const;
export type MinLength = (typeof MIN_LENGTHS)[number];

function loadMinLength(): MinLength {
  try {
    const v = Number(localStorage.getItem("library.minLength") ?? 30);
    return (MIN_LENGTHS as readonly number[]).includes(v) ? (v as MinLength) : 30;
  } catch {
    return 30;
  }
}

/** True if a track passes the short-track filter. */
export function longEnough(t: Track, min: number = useLibrary.getState().minLength): boolean {
  return min <= 0 || t.duration >= min;
}

function derive(allTracks: Track[], roots: string[], min: number) {
  const tracks = min > 0 ? allTracks.filter((t) => longEnough(t, min)) : allTracks;
  const built = build(tracks, roots);
  if (tracks === allTracks) return { tracks, ...built };
  // Lookups stay complete so playlists, the queue and artwork still resolve hidden tracks.
  return {
    tracks,
    ...built,
    byId: new Map(allTracks.map((t) => [t.id, t])),
    byPath: new Map(allTracks.map((t) => [t.path, t])),
  };
}

export const useLibrary = create<LibraryStore>((set, get) => ({
  loaded: false,
  allTracks: [],
  tracks: [],
  minLength: loadMinLength(),
  setMinLength: (minLength) => {
    try {
      localStorage.setItem("library.minLength", String(minLength));
    } catch {
      /* ignore */
    }
    const s = get();
    set({ minLength, ...derive(s.allTracks, s.folderRoots, minLength) });
  },
  folderRoots: [],
  byId: new Map(),
  byPath: new Map(),
  folders: new Map(),
  roots: [],
  haystack: [],
  scanning: false,
  progress: null,
  load: async () => {
    const [allTracks, folders] = await Promise.all([api.getTracks(), api.getLibraryFolders()]);
    const folderRoots = folders.map((f) => f.path);
    set({ loaded: true, allTracks, folderRoots, ...derive(allTracks, folderRoots, get().minLength) });
  },
}));

/**
 * Artwork id for a track as the library currently knows it, so hiding artwork applies
 * immediately everywhere (queue items and the player hold older copies of the track).
 */
export function liveCoverId(t: Track | null | undefined): string | undefined {
  if (!t) return undefined;
  const lib = useLibrary.getState();
  const known = t.id > 0 ? lib.byId.get(t.id) : lib.byPath.get(t.path);
  return known ? known.coverId : t.coverId;
}

/** Hook version of liveCoverId (re-renders when the library reloads). */
export function useLiveCover(t: Track | null | undefined): string | undefined {
  const byId = useLibrary((s) => s.byId);
  const byPath = useLibrary((s) => s.byPath);
  if (!t) return undefined;
  const known = t.id > 0 ? byId.get(t.id) : byPath.get(t.path);
  return known ? known.coverId : t.coverId;
}

/** GIF or video artwork for a track, as the library currently knows it (see liveCoverId). */
export function liveMotionId(t: Track | null | undefined): string | undefined {
  if (!t) return undefined;
  const lib = useLibrary.getState();
  const known = t.id > 0 ? lib.byId.get(t.id) : lib.byPath.get(t.path);
  return known ? known.motionId : t.motionId;
}

/** Hook version of liveMotionId. */
export function useLiveMotion(t: Track | null | undefined): string | undefined {
  const byId = useLibrary((s) => s.byId);
  const byPath = useLibrary((s) => s.byPath);
  if (!t) return undefined;
  const known = t.id > 0 ? byId.get(t.id) : byPath.get(t.path);
  return known ? known.motionId : t.motionId;
}

/** Tracks matching every whitespace-separated token of `q`. */
export function searchTracks(q: string, limit = 500): Track[] {
  const { tracks, haystack } = useLibrary.getState();
  const tokens = q.toLowerCase().split(/\s+/).filter(Boolean);
  if (!tokens.length) return [];
  const out: Track[] = [];
  for (let i = 0; i < tracks.length && out.length < limit; i++) {
    const h = haystack[i];
    if (tokens.every((tok) => h.includes(tok))) out.push(tracks[i]);
  }
  return out;
}

let initialized = false;
export async function initLibrary() {
  if (initialized) return;
  initialized = true;
  await listen<ScanProgress>("library:progress", (e) => useLibrary.setState({ progress: e.payload, scanning: true }));
  await listen("library:scan-started", () => useLibrary.setState({ scanning: true }));
  await listen<ScanSummary>("library:changed", async (e) => {
    useLibrary.setState({ scanning: false, progress: null });
    const s = e.payload;
    await useLibrary.getState().load();
    if (s.added + s.removed + s.updated > 0) {
      const parts = [];
      if (s.added) parts.push(`${s.added} added`);
      if (s.updated) parts.push(`${s.updated} updated`);
      if (s.removed) parts.push(`${s.removed} removed`);
      usePlayer.getState().pushToast("info", `Library updated: ${parts.join(", ")}.`);
    }
    if (s.errors > 0) usePlayer.getState().pushToast("error", `${s.errors} file(s) could not be read.`);
  });
  useLibrary.setState({ scanning: await api.isScanning() });
  await useLibrary.getState().load();
}
