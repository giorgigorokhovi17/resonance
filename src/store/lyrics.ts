import { create } from "zustand";
import { api } from "../lib/api";
import type { Lyrics } from "../lib/types";

interface LyricsStore {
  /** By file path; `null` = the song has none. */
  byPath: Map<string, Lyrics | null>;
  /** Songs being looked up online. */
  searching: Set<string>;
  /** Why an online search found nothing, by path. */
  notFound: Map<string, string>;
}

const useLyricsStore = create<LyricsStore>(() => ({ byPath: new Map(), searching: new Set(), notFound: new Map() }));

const loading = new Set<string>();

function put(path: string, lyrics: Lyrics | null) {
  useLyricsStore.setState((s) => ({ byPath: new Map(s.byPath).set(path, lyrics) }));
}

function load(path: string) {
  if (loading.has(path) || useLyricsStore.getState().byPath.has(path)) return;
  loading.add(path);
  api
    .getLyrics(path)
    .catch(() => null)
    .then((l) => put(path, l))
    .finally(() => loading.delete(path));
}

/** Looks the song's lyrics up on LRCLIB. Shared by the lyrics panel and the lyrics window. */
export async function searchLyrics(path: string) {
  const st = useLyricsStore.getState();
  if (st.searching.has(path)) return;
  const notFound = new Map(st.notFound);
  notFound.delete(path);
  useLyricsStore.setState({ searching: new Set(st.searching).add(path), notFound });
  try {
    const r = await api.searchLyrics(path);
    if (r.lyrics) put(path, r.lyrics);
    else useLyricsStore.setState((s) => ({ notFound: new Map(s.notFound).set(path, r.message ?? "No lyrics found for this song.") }));
  } catch (e) {
    useLyricsStore.setState((s) => ({ notFound: new Map(s.notFound).set(path, String(e)) }));
  } finally {
    useLyricsStore.setState((s) => {
      const searching = new Set(s.searching);
      searching.delete(path);
      return { searching };
    });
  }
}

/** Forgets lyrics found online (when they're wrong) and shows the song without them. */
export async function removeFoundLyrics(path: string) {
  await api.removeFoundLyrics(path);
  useLyricsStore.setState((s) => {
    const byPath = new Map(s.byPath);
    byPath.delete(path);
    return { byPath };
  });
  load(path);
}

/** A song's lyrics: `undefined` while loading, `null` when it has none. */
export function useLyrics(path: string | null) {
  const lyrics = useLyricsStore((s) => (path ? s.byPath.get(path) : null));
  const searching = useLyricsStore((s) => (path ? s.searching.has(path) : false));
  const notFound = useLyricsStore((s) => (path ? s.notFound.get(path) ?? null : null));
  if (path && lyrics === undefined) load(path);
  return { lyrics, searching, notFound };
}
