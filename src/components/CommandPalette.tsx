import {
  BarChart3,
  Download,
  FileText,
  FolderTree,
  LayoutGrid,
  ListMusic,
  ListOrdered,
  Maximize2,
  Moon,
  Music2,
  Pause,
  Play,
  RefreshCw,
  Repeat,
  Repeat1,
  Search,
  Settings2,
  Shuffle,
  SkipBack,
  SkipForward,
  SlidersHorizontal,
  Sun,
  Timer,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useIsDark } from "../hooks/useAppearance";
import { playNext, playShuffled, playTracks } from "../lib/actions";
import { api } from "../lib/api";
import { displayArtist, fmtTime } from "../lib/format";
import { fuzzyScore } from "../lib/fuzzy";
import type { Playlist, Track } from "../lib/types";
import { liveCoverId, searchTracks, useLibrary } from "../store/library";
import { usePlayer } from "../store/player";
import { usePlaylists } from "../store/playlists";
import { setSleepTimer } from "../store/sleep";
import { useUi, type View } from "../store/ui";
import { saveAppearance } from "../views/settings/Visual";
import { Cover } from "./Cover";

const RECENT_KEY = "palette.recent";
const MAX_RECENT = 5;

type PageId = "songs" | "tracks" | "playlists" | "folders" | "sources" | "stats" | "queue" | "eq" | "settings";
type ActionId =
  | "play-pause"
  | "next"
  | "previous"
  | "shuffle"
  | "repeat"
  | "shuffle-all"
  | "now-playing"
  | "lyrics"
  | "sleep-30"
  | "toggle-theme"
  | "rescan";

type RecentRef =
  | { kind: "song"; id: number }
  | { kind: "playlist"; id: number }
  | { kind: "page"; id: PageId }
  | { kind: "action"; id: ActionId };

interface PaletteItem {
  key: string;
  ref: RecentRef;
  label: string;
  subtitle?: string;
  meta?: string;
  shortcut?: string;
  keywords?: string;
  icon: ReactNode;
  track?: Track;
  run: () => void;
}

interface PaletteGroup {
  title: string;
  items: PaletteItem[];
}

interface PageDef {
  id: PageId;
  label: string;
  keywords: string;
  view: View;
  icon: ReactNode;
}

const PAGES: PageDef[] = [
  { id: "songs", label: "Songs", keywords: "library grid albums", view: { kind: "songs" }, icon: <LayoutGrid size={16} /> },
  { id: "tracks", label: "Tracks", keywords: "all songs list", view: { kind: "tracks" }, icon: <Music2 size={16} /> },
  { id: "playlists", label: "Playlists", keywords: "collections", view: { kind: "playlists" }, icon: <ListMusic size={16} /> },
  { id: "folders", label: "Folders", keywords: "files directories", view: { kind: "folders" }, icon: <FolderTree size={16} /> },
  { id: "sources", label: "Sources", keywords: "soulseek youtube download", view: { kind: "sources" }, icon: <Download size={16} /> },
  { id: "stats", label: "Your stats", keywords: "listening history", view: { kind: "stats" }, icon: <BarChart3 size={16} /> },
  { id: "queue", label: "Queue", keywords: "up next", view: { kind: "queue" }, icon: <ListOrdered size={16} /> },
  { id: "eq", label: "Equalizer", keywords: "eq bands audio", view: { kind: "eq" }, icon: <SlidersHorizontal size={16} /> },
  { id: "settings", label: "Settings", keywords: "preferences options", view: { kind: "settings" }, icon: <Settings2 size={16} /> },
];

function sameRef(a: RecentRef, b: RecentRef): boolean {
  return a.kind === b.kind && a.id === b.id;
}

function loadRecent(): RecentRef[] {
  try {
    const raw = localStorage.getItem(RECENT_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter(
        (x): x is RecentRef =>
          !!x &&
          typeof x === "object" &&
          ((x.kind === "song" && typeof x.id === "number") ||
            (x.kind === "playlist" && typeof x.id === "number") ||
            (x.kind === "page" && typeof x.id === "string") ||
            (x.kind === "action" && typeof x.id === "string")),
      )
      .slice(0, MAX_RECENT);
  } catch {
    return [];
  }
}

function saveRecent(ref: RecentRef) {
  try {
    const next = [ref, ...loadRecent().filter((r) => !sameRef(r, ref))].slice(0, MAX_RECENT);
    localStorage.setItem(RECENT_KEY, JSON.stringify(next));
  } catch {
    /* ignore */
  }
}

function songItem(track: Track, keyPrefix = ""): PaletteItem {
  return {
    key: `${keyPrefix}song:${track.id}`,
    ref: { kind: "song", id: track.id },
    label: track.title,
    subtitle: displayArtist(track),
    meta: fmtTime(track.duration),
    icon: <Cover coverId={liveCoverId(track)} className="h-7 w-7 shrink-0" rounded="rounded-md" iconSize={14} />,
    track,
    run: () => void playTracks([track]),
  };
}

function playlistItem(playlist: Playlist, keyPrefix = ""): PaletteItem {
  return {
    key: `${keyPrefix}playlist:${playlist.id}`,
    ref: { kind: "playlist", id: playlist.id },
    label: playlist.name,
    meta: `${playlist.trackCount} ${playlist.trackCount === 1 ? "song" : "songs"}`,
    icon: (
      <Cover
        coverId={playlist.coverId ?? playlist.previewCovers[0]}
        className="h-7 w-7 shrink-0"
        rounded="rounded-md"
        iconSize={14}
      />
    ),
    run: () => useUi.getState().setView({ kind: "playlist", id: playlist.id }),
  };
}

function pageItem(p: PageDef, keyPrefix = ""): PaletteItem {
  return {
    key: `${keyPrefix}page:${p.id}`,
    ref: { kind: "page", id: p.id },
    label: p.label,
    keywords: p.keywords,
    icon: <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-hover">{p.icon}</span>,
    run: () => useUi.getState().setView(p.view),
  };
}

function scoreItem(query: string, label: string, keywords?: string): number | null {
  const s1 = fuzzyScore(query, label);
  const s2 = keywords ? fuzzyScore(query, keywords) : null;
  if (s1 === null && s2 === null) return null;
  if (s1 !== null && s2 !== null) return Math.max(s1 + 8, s2);
  return s1 !== null ? s1 + 8 : s2;
}

export function CommandPalette() {
  const open = useUi((s) => s.commandPaletteOpen);
  if (!open) return null;
  return <CommandPaletteDialog />;
}

function CommandPaletteDialog() {
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const rowRefs = useRef<(HTMLButtonElement | null)[]>([]);

  const playlists = usePlaylists((s) => s.playlists);
  const byId = useLibrary((s) => s.byId);
  const playing = usePlayer((s) => s.status?.state === "playing");
  const shuffle = usePlayer((s) => s.queue.shuffle);
  const repeat = usePlayer((s) => s.queue.repeat);
  const nowPlaying = useUi((s) => s.nowPlaying);
  const lyricsOpen = useUi((s) => s.lyricsOpen);
  const dark = useIsDark();

  const close = useCallback(() => useUi.getState().setCommandPaletteOpen(false), []);

  useEffect(() => {
    api.pauseHotkeys(true).catch(() => {});
    inputRef.current?.focus();
    return () => {
      api.pauseHotkeys(false).catch(() => {});
    };
  }, []);

  const actions = useMemo<PaletteItem[]>(() => {
    const iconBox = (icon: ReactNode) => (
      <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-hover">{icon}</span>
    );
    const repeatNext = repeat === "off" ? "all" : repeat === "all" ? "one" : "off";
    const repeatHint = repeat === "off" ? "Off → All" : repeat === "all" ? "All → One" : "One → Off";
    return [
      {
        key: "action:play-pause",
        ref: { kind: "action", id: "play-pause" },
        label: playing ? "Pause" : "Play",
        keywords: "play pause toggle playback",
        shortcut: "Space",
        icon: iconBox(playing ? <Pause size={16} /> : <Play size={16} />),
        run: () => void api.togglePlay(),
      },
      {
        key: "action:next",
        ref: { kind: "action", id: "next" },
        label: "Next song",
        keywords: "next skip forward track",
        shortcut: "Ctrl+→",
        icon: iconBox(<SkipForward size={16} />),
        run: () => void api.next(),
      },
      {
        key: "action:previous",
        ref: { kind: "action", id: "previous" },
        label: "Previous song",
        keywords: "previous prev back track",
        shortcut: "Ctrl+←",
        icon: iconBox(<SkipBack size={16} />),
        run: () => void api.previous(),
      },
      {
        key: "action:shuffle",
        ref: { kind: "action", id: "shuffle" },
        label: shuffle ? "Turn shuffle off" : "Turn shuffle on",
        keywords: "shuffle toggle random",
        shortcut: "S",
        icon: iconBox(<Shuffle size={16} />),
        run: () => void api.setShuffle(!shuffle),
      },
      {
        key: "action:repeat",
        ref: { kind: "action", id: "repeat" },
        label: "Repeat mode",
        subtitle: repeatHint,
        keywords: "repeat loop mode off all one",
        shortcut: "R",
        icon: iconBox(repeat === "one" ? <Repeat1 size={16} /> : <Repeat size={16} />),
        run: () => void api.setRepeat(repeatNext),
      },
      {
        key: "action:shuffle-all",
        ref: { kind: "action", id: "shuffle-all" },
        label: "Shuffle all songs",
        keywords: "shuffle all library play random",
        icon: iconBox(<Shuffle size={16} />),
        run: () => void playShuffled(useLibrary.getState().tracks),
      },
      {
        key: "action:now-playing",
        ref: { kind: "action", id: "now-playing" },
        label: nowPlaying ? "Close now playing" : "Open now playing",
        keywords: "now playing fullscreen cover",
        shortcut: "F",
        icon: iconBox(<Maximize2 size={16} />),
        run: () => useUi.getState().toggleNowPlaying(),
      },
      {
        key: "action:lyrics",
        ref: { kind: "action", id: "lyrics" },
        label: lyricsOpen ? "Hide lyrics" : "Show lyrics",
        keywords: "lyrics words",
        shortcut: "L",
        icon: iconBox(<FileText size={16} />),
        run: () => useUi.getState().setLyricsOpen(!lyricsOpen),
      },
      {
        key: "action:sleep-30",
        ref: { kind: "action", id: "sleep-30" },
        label: "Sleep timer: 30 min",
        keywords: "sleep timer 30 minutes pause",
        icon: iconBox(<Timer size={16} />),
        run: () => {
          setSleepTimer(30)
            .then(() => usePlayer.getState().pushToast("info", "Sleep timer set for 30 min."))
            .catch((e) => usePlayer.getState().pushToast("error", String(e)));
        },
      },
      {
        key: "action:toggle-theme",
        ref: { kind: "action", id: "toggle-theme" },
        label: dark ? "Switch to light mode" : "Switch to dark mode",
        keywords: "theme dark light mode appearance switch",
        icon: iconBox(dark ? <Sun size={16} /> : <Moon size={16} />),
        run: () => void saveAppearance({ theme: dark ? "light" : "dark" }),
      },
      {
        key: "action:rescan",
        ref: { kind: "action", id: "rescan" },
        label: "Rescan library",
        keywords: "rescan scan refresh library folders",
        icon: iconBox(<RefreshCw size={16} />),
        run: () => {
          api.rescanLibrary(false).catch((e) => usePlayer.getState().pushToast("error", String(e)));
        },
      },
    ];
  }, [playing, shuffle, repeat, nowPlaying, lyricsOpen, dark]);

  const recentRefs = useMemo(() => loadRecent(), []);

  const groups = useMemo<PaletteGroup[]>(() => {
    const q = query.trim();
    if (!q) {
      const out: PaletteGroup[] = [];
      const recentItems: PaletteItem[] = [];
      for (const ref of recentRefs) {
        if (ref.kind === "song") {
          const t = byId.get(ref.id);
          if (t) recentItems.push(songItem(t, "recent:"));
        } else if (ref.kind === "playlist") {
          const p = playlists.find((x) => x.id === ref.id);
          if (p) recentItems.push(playlistItem(p, "recent:"));
        } else if (ref.kind === "page") {
          const p = PAGES.find((x) => x.id === ref.id);
          if (p) recentItems.push(pageItem(p, "recent:"));
        } else if (ref.kind === "action") {
          const a = actions.find((x) => x.ref.kind === "action" && x.ref.id === ref.id);
          if (a) recentItems.push({ ...a, key: `recent:${a.key}` });
        }
      }
      if (recentItems.length) out.push({ title: "Recent", items: recentItems });
      out.push({ title: "Actions", items: actions });
      out.push({ title: "Pages", items: PAGES.map((p) => pageItem(p)) });
      return out;
    }

    const out: PaletteGroup[] = [];

    const matchedSongs = searchTracks(q, 6).map((t) => songItem(t));
    if (matchedSongs.length) out.push({ title: "Songs", items: matchedSongs });

    const matchedPlaylists = playlists
      .map((p) => ({ p, score: fuzzyScore(q, p.name) }))
      .filter((x): x is { p: Playlist; score: number } => x.score !== null)
      .sort((a, b) => b.score - a.score)
      .slice(0, 6)
      .map((x) => playlistItem(x.p));
    if (matchedPlaylists.length) out.push({ title: "Playlists", items: matchedPlaylists });

    const matchedPages = PAGES.map((p) => ({ p, score: scoreItem(q, p.label, p.keywords) }))
      .filter((x): x is { p: PageDef; score: number } => x.score !== null)
      .sort((a, b) => b.score - a.score)
      .map((x) => pageItem(x.p));
    if (matchedPages.length) out.push({ title: "Pages", items: matchedPages });

    const matchedActions = actions
      .map((a) => ({ a, score: scoreItem(q, a.label, a.keywords) }))
      .filter((x): x is { a: PaletteItem; score: number } => x.score !== null)
      .sort((a, b) => b.score - a.score)
      .map((x) => x.a);
    if (matchedActions.length) out.push({ title: "Actions", items: matchedActions });

    return out;
  }, [query, recentRefs, byId, playlists, actions]);

  const flat = useMemo(() => groups.flatMap((g) => g.items), [groups]);
  const activeIndex = flat.length ? Math.min(selected, flat.length - 1) : -1;
  const activeItem = activeIndex >= 0 ? flat[activeIndex] : null;

  useEffect(() => {
    setSelected(0);
  }, [query]);

  useEffect(() => {
    if (activeIndex >= 0) {
      rowRefs.current[activeIndex]?.scrollIntoView({ block: "nearest" });
    }
  }, [activeIndex]);

  const execute = useCallback(
    (item: PaletteItem, asPlayNext = false) => {
      saveRecent(item.ref);
      close();
      if (asPlayNext && item.track) void playNext([item.track]);
      else item.run();
    },
    [close],
  );

  let rowIdx = 0;

  return (
    <div
      className="fixed inset-0 z-[115] flex items-start justify-center bg-black/50 px-4 pt-[14vh] backdrop-blur-sm"
      onMouseDown={close}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        className="animate-palette-in flex max-h-[68vh] w-full max-w-[560px] flex-col overflow-hidden rounded-xl bg-panel shadow-2xl ring-1 ring-line"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape" || (e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey && e.key.toLowerCase() === "k")) {
            e.preventDefault();
            e.stopPropagation();
            close();
            return;
          }
          if (e.key === "ArrowDown") {
            e.preventDefault();
            if (flat.length) setSelected((i) => (i + 1) % flat.length);
          } else if (e.key === "ArrowUp") {
            e.preventDefault();
            if (flat.length) setSelected((i) => (i - 1 + flat.length) % flat.length);
          } else if (e.key === "Enter" && activeItem) {
            e.preventDefault();
            execute(activeItem, e.ctrlKey);
          }
          e.stopPropagation();
        }}
      >
        <div className="flex items-center gap-2.5 border-b border-line px-3.5 py-2.5">
          <Search size={16} className="shrink-0 text-faint" />
          <input
            ref={inputRef}
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search songs, playlists, pages, or actions…"
            spellCheck={false}
            aria-label="Command palette search"
            className="min-w-0 flex-1 bg-transparent text-[14px] text-fg outline-none placeholder:text-faint"
          />
          {query && (
            <button
              type="button"
              aria-label="Clear search"
              onClick={() => {
                setQuery("");
                inputRef.current?.focus();
              }}
              className="rounded p-1 text-faint hover:bg-hover hover:text-fg"
            >
              <X size={14} />
            </button>
          )}
          <kbd className="rounded-md bg-bg px-1.5 py-0.5 text-[10.5px] font-medium text-faint ring-1 ring-line">Esc</kbd>
        </div>

        <div role="listbox" aria-label="Results" className="min-h-0 flex-1 overflow-y-auto p-1.5">
          {groups.length === 0 ? (
            <div className="px-3 py-10 text-center text-[13px] text-muted">No matches for “{query.trim()}”.</div>
          ) : (
            groups.map((group) => (
              <div key={group.title} className="mb-1.5 last:mb-0">
                <div className="font-display px-2.5 pb-1 pt-2 text-[11px] font-semibold uppercase tracking-wider text-faint">
                  {group.title}
                </div>
                {group.items.map((item) => {
                  const idx = rowIdx++;
                  const isActive = idx === activeIndex;
                  return (
                    <button
                      key={item.key}
                      ref={(el) => {
                        rowRefs.current[idx] = el;
                      }}
                      type="button"
                      role="option"
                      aria-selected={isActive}
                      onMouseMove={() => {
                        if (selected !== idx) setSelected(idx);
                      }}
                      onClick={(e) => execute(item, e.ctrlKey)}
                      className={`flex w-full items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-left transition-colors ${
                        isActive ? "bg-hover text-fg" : "text-muted hover:text-fg"
                      }`}
                    >
                      <span className={isActive ? "text-accent" : "text-muted"}>{item.icon}</span>
                      <span className="min-w-0 flex-1">
                        <span className="block truncate text-[13px] font-medium text-fg">{item.label}</span>
                        {item.subtitle && <span className="block truncate text-[11.5px] text-muted">{item.subtitle}</span>}
                      </span>
                      {item.meta && <span className="tabular shrink-0 text-[11.5px] text-faint">{item.meta}</span>}
                      {item.shortcut && (
                        <kbd className="shrink-0 rounded-md bg-bg px-1.5 py-0.5 text-[11px] font-medium text-muted ring-1 ring-line">
                          {item.shortcut}
                        </kbd>
                      )}
                    </button>
                  );
                })}
              </div>
            ))
          )}
        </div>

        <div className="flex flex-wrap items-center justify-between gap-2 border-t border-line px-3.5 py-2 text-[11px] text-faint">
          <div className="flex items-center gap-3">
            <span>
              <kbd className="rounded bg-bg px-1 py-0.5 text-[10px] text-muted ring-1 ring-line">↑↓</kbd> Move
            </span>
            <span>
              <kbd className="rounded bg-bg px-1 py-0.5 text-[10px] text-muted ring-1 ring-line">Enter</kbd>{" "}
              {activeItem?.track ? "Play" : "Run"}
            </span>
            {activeItem?.track && (
              <span>
                <kbd className="rounded bg-bg px-1 py-0.5 text-[10px] text-muted ring-1 ring-line">Ctrl+Enter</kbd> Play next
              </span>
            )}
          </div>
          <span>
            <kbd className="rounded bg-bg px-1 py-0.5 text-[10px] text-muted ring-1 ring-line">Ctrl+K</kbd> Toggle
          </span>
        </div>
      </div>
    </div>
  );
}
