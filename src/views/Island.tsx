import { emitTo, listen } from "@tauri-apps/api/event";
import { availableMonitors, getCurrentWindow, LogicalSize, primaryMonitor } from "@tauri-apps/api/window";
import { Check, FolderOpen, Pause, Play, Repeat, Repeat1, Settings2, Shuffle, SkipBack, SkipForward, X } from "lucide-react";
import { useEffect, useRef, useState, type MouseEvent, type ReactNode } from "react";
import { Cover } from "../components/Cover";
import { api } from "../lib/api";
import { displayArtist, fmtTime } from "../lib/format";
import {
  areaAt,
  collapsedRect,
  FALLBACK_PALETTE,
  fixedPalette,
  grownRect,
  ISLAND_PALETTES,
  overlaps,
  POSITION_LABELS,
  type Rect,
} from "../lib/island";
import { coverPalette, type Palette } from "../lib/palette";
import type { IslandOptions, IslandPosition, Playlist, RepeatMode } from "../lib/types";
import { useLyrics } from "../store/lyrics";
import { initPlayer, livePosition, usePlayer } from "../store/player";
import { playPlaylist } from "./PlaylistView";

type Size = { w: number; h: number };
/**
 * The small bar, the card shown when a new song starts, the full island on hover, and the
 * playlist picker (the folder button).
 */
type Level = "small" | "card" | "big" | "list";
const RANK: Record<Level, number> = { small: 0, card: 1, big: 2, list: 3 };
/**
 * Where the island is at each size (physical px), grown from where the small bar sits, and the
 * window: big enough for all of them, so it never resizes while the pill grows and shrinks.
 */
type Geo = { rects: Record<Level, Rect>; window: Rect; sf: number; up: boolean };

/** Window sizes (logical px). The pill sits inside with room for its glow. */
const CARD = { w: 400, h: 98 };
const BIG = { w: 500, h: 186 };
const LIST = { w: 420, h: 330 };
/** Space around the pill inside the window. */
const SIDE = 10;
const EDGE = 6;
/** How long the card stays after a new song starts. */
const PEEK_MS = 4000;
/** The mouse moves this far (px) with the button down before a press becomes a drag. */
const DRAG_START = 4;
const NEXT_REPEAT: Record<RepeatMode, RepeatMode> = { off: "all", all: "one", one: "off" };

const win = getCurrentWindow();

/** The small bar fits what it shows: cover, then title/artist, then controls or level bars. */
function smallSize(o: IslandOptions): Size {
  const text = o.showTitle || o.showArtist;
  const controls = o.showControls ? (o.showShuffleRepeat ? 148 : 92) : 26;
  return { w: 68 + (text ? 220 : 0) + controls + (o.showPlaylists ? 28 : 0), h: 58 };
}

function levelSize(level: Level, small: Size): Size {
  return level === "list" ? LIST : level === "big" ? BIG : level === "card" ? CARD : small;
}

/** The pill's size at each level. */
function pillSize(level: Level, small: Size): Size {
  const s = levelSize(level, small);
  return { w: s.w - 2 * SIDE, h: level === "small" ? s.h - 18 : s.h - 22 };
}

/**
 * Where the pill goes inside the window at `level` (logical px): where the island would be at
 * that size, so it grows from and shrinks back to exactly where the small bar sits.
 */
function pillSpot(geo: Geo | null, level: Level, small: Size) {
  const box = pillSize(level, small);
  const own = levelSize(level, small);
  const r = geo?.rects[level];
  const dx = geo && r ? (r.x - geo.window.x) / geo.sf : (BIG.w - own.w) / 2;
  const dy = geo && r ? (r.y - geo.window.y) / geo.sf : 0;
  const up = geo?.up ?? false;
  return { left: dx + SIDE, top: dy + (up ? own.h - box.h - EDGE : EDGE), w: box.w, h: box.h };
}

function options(): IslandOptions | null {
  return usePlayer.getState().settings?.islandOptions ?? null;
}

function save(patch: Partial<IslandOptions>) {
  const o = options();
  if (o) api.updateSettings({ islandOptions: { ...o, ...patch } }).catch((e) => console.error(e));
}

/**
 * Works out where the window goes at each size: the small bar where the options put it, the
 * card and the full island grown from it and kept on screen. In physical pixels so it lands right
 * on screens with different scaling.
 */
async function layout(o: IslandOptions, small: Size, avoid: Rect | null): Promise<Geo | null> {
  const [monitors, primary] = await Promise.all([availableMonitors(), primaryMonitor()]);
  const areas = monitors.map((m) => ({
    rect: { x: m.workArea.position.x, y: m.workArea.position.y, w: m.workArea.size.width, h: m.workArea.size.height } as Rect,
    sf: m.scaleFactor,
    name: m.name,
  }));
  let area: (typeof areas)[number] | undefined;
  if (o.position === "custom" && o.x != null && o.y != null) {
    const r = areaAt(areas.map((a) => a.rect), o.x, o.y);
    area = areas.find((a) => a.rect === r);
  } else {
    // Corners and edges of the chosen screen, else the main one (never just the one it's on now,
    // or it would stay wherever a game sent it).
    area = areas.find((a) => o.screen && a.name === o.screen) ?? areas.find((a) => primary && a.name === primary.name) ?? areas[0];
  }
  if (!area) return null;
  // A full-screen game on this screen: the same spot on another one, so the game keeps its vsync.
  let opts = o;
  const other = avoid && overlaps(area.rect, avoid) ? areas.find((a) => !overlaps(a.rect, avoid)) : undefined;
  if (other) {
    if (o.position === "custom" && o.x != null && o.y != null) {
      const fx = (o.x - area.rect.x) / area.rect.w;
      const fy = (o.y - area.rect.y) / area.rect.h;
      opts = { ...o, x: Math.round(other.rect.x + fx * other.rect.w), y: Math.round(other.rect.y + fy * other.rect.h) };
    }
    area = other;
  }
  const px = (s: Size) => ({ w: Math.round(s.w * area.sf), h: Math.round(s.h * area.sf) });
  const collapsed = collapsedRect(opts, area.rect, px(small));
  const card = grownRect(collapsed, px(CARD), area.rect);
  const big = grownRect(collapsed, px(BIG), area.rect);
  const list = grownRect(collapsed, px(LIST), area.rect);
  const all = [collapsed, card.rect, big.rect, list.rect];
  const x = Math.min(...all.map((r) => r.x));
  const y = Math.min(...all.map((r) => r.y));
  const window = {
    x,
    y,
    w: Math.max(...all.map((r) => r.x + r.w)) - x,
    h: Math.max(...all.map((r) => r.y + r.h)) - y,
  };
  return { rects: { small: collapsed, card: card.rect, big: big.rect, list: list.rect }, window, sf: area.sf, up: big.up };
}

/** The current timed lyric line, if the song has synced lyrics. */
function useLyricLine(path: string | null) {
  const { lyrics } = useLyrics(path);
  const [line, setLine] = useState<string | null>(null);
  useEffect(() => {
    setLine(null);
    if (!lyrics?.synced) return;
    const tick = () => {
      const ms = livePosition() * 1000;
      let i = -1;
      while (i + 1 < lyrics.lines.length && (lyrics.lines[i + 1].timeMs ?? 0) <= ms) i++;
      setLine(i >= 0 ? lyrics.lines[i].text || "♪" : null);
    };
    tick();
    const id = window.setInterval(tick, 250);
    return () => window.clearInterval(id);
  }, [lyrics]);
  return line;
}

/** The yes/no options in the right-click panel. */
const TOGGLES: { key: keyof IslandOptions; text: string }[] = [
  { key: "locked", text: "Lock position" },
  { key: "clickThrough", text: "Let clicks through (hold Alt to use it)" },
  { key: "showTitle", text: "Show the song title" },
  { key: "showArtist", text: "Show the artist" },
  { key: "showControls", text: "Show play controls" },
  { key: "showShuffleRepeat", text: "Show shuffle and repeat" },
  { key: "showPlaylists", text: "Show playlist button" },
  { key: "showProgress", text: "Show the seek bar" },
  { key: "peekOnNewSong", text: "Pop up when a song starts" },
  { key: "expandOnHover", text: "Open when pointed at" },
  { key: "onlyWhenMinimized", text: "Only while Undertone is minimised" },
  { key: "hideInFullscreen", text: "Hide over full-screen games and videos" },
  { key: "moveInFullscreen", text: "Otherwise, move to another screen over them" },
];

/** A panel (playlists, options) closes when the pointer has been away this long, or idle in it this long. */
const PICKER_LEAVE_MS = 700;
const PICKER_IDLE_MS = 10_000;

/**
 * The Dynamic Island: a bar with the playing song, coloured from its cover or a chosen palette,
 * with play controls and a seek bar. A small card shows when a new song starts; pointing at it
 * opens it fully (controls, progress, the current lyric line). Drag it anywhere, right-click it
 * for options, double-click it to open Undertone. When the queue ends it stays, stopped.
 */
export function Island() {
  const track = usePlayer((s) => s.status?.track ?? s.queue.items.find((i) => i.uid === s.queue.currentUid)?.track ?? null);
  const state = usePlayer((s) => s.status?.state ?? "stopped");
  const duration = usePlayer((s) => s.status?.duration || track?.duration || 0);
  const shuffle = usePlayer((s) => s.queue.shuffle);
  const repeat = usePlayer((s) => s.queue.repeat);
  const o = usePlayer((s) => s.settings?.islandOptions ?? null);
  const [fullscreen, setFullscreen] = useState(false);
  /** The screen a full-screen app covers (physical px), when known. */
  const [gameScreen, setGameScreen] = useState<Rect | null>(null);
  const [mainAway, setMainAway] = useState(false);
  const [hover, setHover] = useState(false);
  const [peek, setPeek] = useState(false);
  /** The open panel: the playlist picker or the right-click options; null while closed. */
  const [panel, setPanel] = useState<"playlists" | "options" | null>(null);
  const [lists, setLists] = useState<Playlist[]>([]);
  const pickerTimer = useRef<number>();
  const [geo, setGeo] = useState<Geo | null>(null);
  const geoRef = useRef<Geo | null>(null);
  const [palettes, setPalettes] = useState<{ key: string; p: Palette }[]>([{ key: "none", p: FALLBACK_PALETTE }]);
  const [pos, setPos] = useState(0);
  const [levels, setLevels] = useState<number[]>([]);
  const leave = useRef<number>();
  const lastPath = useRef<string | null>(null);
  const press = useRef<{ x: number; y: number } | null>(null);
  const dragging = useRef(false);
  const line = useLyricLine(track?.path ?? null);

  const showTitle = o?.showTitle ?? true;
  const showArtist = o?.showArtist ?? true;
  const small = o ? smallSize(o) : { w: 320, h: 58 };
  const visible = !!o && !!track && !(fullscreen && o.hideInFullscreen) && (!o.onlyWhenMinimized || mainAway);
  const picking = panel !== null;
  const pill: Level = !visible
    ? "small"
    : picking
      ? "list"
      : hover && o?.expandOnHover
        ? "big"
        : peek && o?.peekOnNewSong
          ? "card"
          : "small";
  const closePicker = () => {
    window.clearTimeout(pickerTimer.current);
    setPanel(null);
  };
  /** (Re)starts the panel's timer: it closes after `ms` unless something happens. */
  const pickerCloseIn = (ms: number) => {
    window.clearTimeout(pickerTimer.current);
    pickerTimer.current = window.setTimeout(() => setPanel(null), ms);
  };
  // Panels are part of the island, so unlike a Windows menu they open with Alt still held, and
  // stay usable once it's let go (click-through mode).
  const openPicker = () => {
    api
      .getPlaylists()
      .then((l) => {
        setLists(l);
        setPanel("playlists");
        pickerCloseIn(PICKER_IDLE_MS);
      })
      .catch((e) => console.error(e));
  };
  const openOptions = () => {
    setPanel("options");
    pickerCloseIn(PICKER_IDLE_MS);
  };
  useEffect(() => {
    api.islandSetPanel(picking).catch(() => {});
  }, [picking]);
  const pick = (id: number, shuffled: boolean) => {
    closePicker();
    playPlaylist(id, 0, shuffled);
  };
  // Hidden (Undertone came back, a game went full screen): the picker doesn't come back open.
  useEffect(() => {
    if (!visible) closePicker();
  }, [visible]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => () => window.clearTimeout(pickerTimer.current), []);
  const avoid = o?.moveInFullscreen && fullscreen ? gameScreen : null;
  const placeKey = o ? `${o.position}|${o.x}|${o.y}|${o.screen}|${small.w}|${avoid ? `${avoid.x},${avoid.y}` : ""}` : "";

  useEffect(() => {
    for (const el of [document.documentElement, document.body]) el.style.background = "transparent";
    initPlayer().catch((e) => console.error(e));
    api.islandMainAway().then(setMainAway).catch(() => {});
    const uns = [
      listen<{ busy: boolean; screen: [number, number, number, number] | null }>("island:fullscreen", (e) => {
        const s = e.payload.screen;
        setFullscreen(e.payload.busy);
        setGameScreen(s ? { x: s[0], y: s[1], w: s[2], h: s[3] } : null);
      }),
      listen<boolean>("island:main-away", (e) => setMainAway(e.payload)),
      listen<number[]>("island:levels", (e) => setLevels(e.payload)),
      // With clicks going through (Alt let go), the pointer can't be seen leaving.
      listen<boolean>("island:clickable", (e) => {
        if (!e.payload) {
          press.current = null;
          setHover(false);
        }
      }),
    ];
    // A drag ends when the window stops moving: keep where it was dropped.
    let settle: number | undefined;
    const unMoved = win.onMoved(() => {
      if (!dragging.current) return;
      window.clearTimeout(settle);
      settle = window.setTimeout(async () => {
        dragging.current = false;
        const g = geoRef.current;
        if (!g) return;
        // The small bar moved as far as the window did.
        const p = await win.outerPosition();
        const s = g.rects.small;
        save({ position: "custom", x: Math.round(s.x + p.x - g.window.x), y: Math.round(s.y + p.y - g.window.y) });
      }, 350);
    });
    return () => {
      window.clearTimeout(settle);
      for (const un of [...uns, unMoved]) un.then((f) => f());
    };
  }, []);

  // Place and show the window, or hide it. It moves only when the island's place or width changes,
  // never while the pill grows and shrinks.
  useEffect(() => {
    if (!visible || !o) {
      win.hide();
      return;
    }
    let alive = true;
    (async () => {
      const g = await layout(o, small, avoid);
      if (!alive) return;
      if (g) {
        const r = g.window;
        await api.islandSetBounds(Math.round(r.x), Math.round(r.y), Math.round(r.w), Math.round(r.h));
      } else {
        await win.setSize(new LogicalSize(BIG.w, BIG.h));
      }
      if (!alive) return;
      geoRef.current = g;
      setGeo(g);
      await win.show();
      // Landing on a screen with other scaling, Windows resizes the window for it (cutting the
      // island off): put it back once that has happened.
      if (g) {
        await new Promise((res) => setTimeout(res, 150));
        const r = g.window;
        const [p, s] = await Promise.all([win.outerPosition(), win.outerSize()]);
        if (alive && (p.x !== r.x || p.y !== r.y || s.width !== r.w || s.height !== r.h)) {
          await api.islandSetBounds(Math.round(r.x), Math.round(r.y), Math.round(r.w), Math.round(r.h));
        }
      }
    })().catch((e) => console.error(e));
    return () => {
      alive = false;
    };
  }, [visible, placeKey]); // eslint-disable-line react-hooks/exhaustive-deps

  // Clicks reach the island only over the pill; elsewhere they go to the app underneath. Growing,
  // that area grows at once; shrinking, it waits for the pill to finish.
  const spot = pillSpot(geo, pill, small);
  const [shapeLevel, setShapeLevel] = useState<Level>("small");
  useEffect(() => {
    if (RANK[pill] >= RANK[shapeLevel]) {
      setShapeLevel(pill);
      return;
    }
    const t = window.setTimeout(() => setShapeLevel(pill), 400);
    return () => window.clearTimeout(t);
  }, [pill]); // eslint-disable-line react-hooks/exhaustive-deps
  const shape = pillSpot(geo, shapeLevel, small);
  useEffect(() => {
    if (!geo) return;
    const sf = geo.sf;
    api.islandSetPill(Math.round(shape.left * sf), Math.round(shape.top * sf), Math.round(shape.w * sf), Math.round(shape.h * sf)).catch(() => {});
  }, [geo, shape.left, shape.top, shape.w, shape.h]);

  // A new song shows the card for a moment.
  useEffect(() => {
    const path = track?.path ?? null;
    if (path && lastPath.current && path !== lastPath.current) {
      setPeek(true);
      const t = window.setTimeout(() => setPeek(false), PEEK_MS);
      lastPath.current = path;
      return () => window.clearTimeout(t);
    }
    lastPath.current = path;
  }, [track?.path]);

  // Colours: a chosen palette, or from the cover. The new gradient fades in over the old one.
  const fixed = o ? fixedPalette(o) : null;
  const fixedKey = fixed?.join(",") ?? "";
  useEffect(() => {
    const add = (key: string, p: Palette) => setPalettes((ps) => [...ps.slice(-1), { key: `${key}-${Date.now()}`, p }]);
    if (fixed) {
      add("fixed", fixed);
      return;
    }
    const id = track?.coverId;
    if (!id) {
      add("none", FALLBACK_PALETTE);
      return;
    }
    let alive = true;
    coverPalette(id).then((p) => alive && add(id, p ?? FALLBACK_PALETTE));
    return () => {
      alive = false;
    };
  }, [track?.coverId, fixedKey]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!visible) return;
    const tick = () => setPos(state === "stopped" ? 0 : livePosition());
    tick();
    // Over a full-screen app the small bar's seek bar moves only every few seconds.
    const id = window.setInterval(tick, pill === "big" ? 250 : fullscreen && pill === "small" ? 3000 : 500);
    return () => window.clearInterval(id);
  }, [visible, pill, state, fullscreen]);

  if (!track || !o) return null;
  const playing = state === "playing";
  const glow = palettes[palettes.length - 1].p[0];
  const onControl = (e: MouseEvent) => !!(e.target as HTMLElement).closest("button, [data-no-drag]");
  const fraction = duration > 0 ? Math.min(1, pos / duration) : 0;
  const seek = (e: MouseEvent<HTMLElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    if (duration > 0) api.seek(Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)) * duration);
  };
  const layer = (level: Level) => ({
    opacity: pill === level ? 1 : 0,
    pointerEvents: pill === level ? ("auto" as const) : ("none" as const),
    transitionDelay: pill === level && level !== "small" ? "120ms" : "0ms",
  });
  const ease = "380ms cubic-bezier(.2,.9,.25,1.15)";
  const repeatLabel = repeat === "off" ? "Repeat: off" : repeat === "all" ? "Repeat: all" : "Repeat: this song";
  const RepeatIcon = repeat === "one" ? Repeat1 : Repeat;

  return (
    <div className="relative h-full w-full">
      <style>{`
        @keyframes island-drift { 0%,100% { background-position: 0% 50%; } 50% { background-position: 100% 50%; } }
        @keyframes island-fade { from { opacity: 0; } to { opacity: 1; } }
      `}</style>
      <div
        onMouseEnter={() => {
          window.clearTimeout(leave.current);
          setHover(true);
          if (picking) pickerCloseIn(PICKER_IDLE_MS);
        }}
        onMouseLeave={() => {
          press.current = null;
          leave.current = window.setTimeout(() => setHover(false), 350);
          if (picking) pickerCloseIn(PICKER_LEAVE_MS);
        }}
        onMouseDown={(e) => {
          press.current = e.button === 0 && !o.locked && !onControl(e) ? { x: e.screenX, y: e.screenY } : null;
        }}
        onMouseMove={(e) => {
          if (picking) pickerCloseIn(PICKER_IDLE_MS);
          const p = press.current;
          if (!p) return;
          if (!(e.buttons & 1)) {
            press.current = null;
          } else if (Math.hypot(e.screenX - p.x, e.screenY - p.y) > DRAG_START) {
            press.current = null;
            dragging.current = true;
            win.startDragging().catch(() => (dragging.current = false));
          }
        }}
        onMouseUp={() => (press.current = null)}
        onDoubleClick={(e) => {
          if (!onControl(e)) api.showMainWindow();
        }}
        onContextMenu={(e) => {
          e.preventDefault();
          if (panel === "options") closePicker();
          else openOptions();
        }}
        className={`absolute select-none overflow-hidden text-white ${o.locked ? "" : "cursor-grab active:cursor-grabbing"}`}
        style={{
          left: spot.left,
          top: spot.top,
          width: spot.w,
          height: spot.h,
          borderRadius: pill === "big" || pill === "list" ? 28 : pill === "card" ? 24 : 22,
          boxShadow: `0 10px 30px -10px ${glow}, 0 0 0 1px rgb(255 255 255 / 0.08) inset`,
          transition: `left ${ease}, top ${ease}, width ${ease}, height ${ease}, border-radius 380ms, box-shadow 600ms`,
        }}
      >
        {palettes.map(({ key, p }) => (
          <div
            key={key}
            className="absolute inset-0"
            style={{
              background: `radial-gradient(90% 140% at 0% 0%, ${p[1]}, transparent 70%), radial-gradient(90% 140% at 100% 100%, ${p[2]}, transparent 70%), linear-gradient(115deg, ${p[0]}, ${p[1]}, ${p[2]}, ${p[0]})`,
              backgroundSize: "100% 100%, 100% 100%, 300% 100%",
              animation: `island-fade 700ms ease-out both${playing && !fullscreen ? ", island-drift 14s ease-in-out infinite" : ""}`,
            }}
          />
        ))}
        <div className="absolute inset-0 bg-black/25" />

        {/* Small bar */}
        <div className="absolute inset-0 transition-opacity duration-200" style={layer("small")}>
          <div className="flex h-full items-center gap-2.5 px-2.5">
            <Cover coverId={track.coverId} className="h-7 w-7 shrink-0" rounded="rounded-full" iconSize={12} />
            {showTitle || showArtist ? <TitleLine title={showTitle ? track.title : null} artist={showArtist ? displayArtist(track) : null} /> : null}
            <div className="ml-auto flex shrink-0 items-center gap-0.5">
              {o.showPlaylists && (
                <SmallBtn label="Play a playlist" onClick={openPicker}>
                  <FolderOpen size={13} />
                </SmallBtn>
              )}
              {o.showControls ? (
                <>
                {o.showShuffleRepeat && (
                  <SmallBtn label={shuffle ? "Shuffle: on" : "Shuffle: off"} active={shuffle} onClick={() => api.setShuffle(!shuffle)}>
                    <Shuffle size={13} />
                  </SmallBtn>
                )}
                <SmallBtn label="Previous" onClick={() => api.previous()}>
                  <SkipBack size={14} fill="currentColor" />
                </SmallBtn>
                <SmallBtn label={playing ? "Pause" : "Play"} onClick={() => api.togglePlay()}>
                  {playing ? <Pause size={15} fill="currentColor" strokeWidth={0} /> : <Play size={15} fill="currentColor" strokeWidth={0} className="translate-x-px" />}
                </SmallBtn>
                <SmallBtn label="Next" onClick={() => api.next()}>
                  <SkipForward size={14} fill="currentColor" />
                </SmallBtn>
                {o.showShuffleRepeat && (
                  <SmallBtn label={repeatLabel} active={repeat !== "off"} onClick={() => api.setRepeat(NEXT_REPEAT[repeat])}>
                    <RepeatIcon size={13} />
                  </SmallBtn>
                )}
                </>
              ) : (
                <span className="ml-1">
                  <Bars levels={levels} />
                </span>
              )}
            </div>
          </div>
          {o.showProgress && (
            <div
              data-no-drag
              title={`${fmtTime(pos)} / ${fmtTime(duration)}`}
              onClick={seek}
              className="group absolute inset-x-4 bottom-0 flex h-2.5 cursor-pointer items-end pb-[3px]"
            >
              <div className="relative h-[2.5px] w-full overflow-hidden rounded-full bg-white/20 transition-[height] group-hover:h-[4px]">
                <div className="absolute inset-y-0 left-0 rounded-full bg-white/85" style={{ width: `${fraction * 100}%` }} />
              </div>
            </div>
          )}
        </div>

        {/* New song card */}
        <div className="absolute inset-0 flex items-center gap-3 px-3 transition-opacity duration-300" style={layer("card")}>
          <Cover coverId={track.coverId} className="h-[52px] w-[52px] shrink-0 shadow-lg" rounded="rounded-xl" iconSize={18} />
          <div className="min-w-0 flex-1">
            <div className="text-[10px] font-semibold uppercase tracking-wider text-white/60">Now playing</div>
            <div className="truncate text-[14px] font-semibold drop-shadow">{track.title}</div>
            <div className="truncate text-[12px] text-white/75">{displayArtist(track)}</div>
            <div className="mt-1.5 h-1 overflow-hidden rounded-full bg-white/20">
              <div className="h-full rounded-full bg-white/85" style={{ width: `${fraction * 100}%` }} />
            </div>
          </div>
          <Bars levels={levels} />
        </div>

        {/* Options (right-click) */}
        {panel === "options" && (
          <div className="absolute inset-0 flex flex-col px-3 pb-3 pt-3.5 transition-opacity duration-300" style={layer("list")}>
            <div className="flex items-center gap-2 px-1.5 pb-2">
              <Settings2 size={15} className="text-white/80" />
              <span className="flex-1 text-[13.5px] font-semibold drop-shadow">Island options</span>
              <button aria-label="Close" title="Close" onClick={closePicker} className="ml-1 rounded-full p-1 text-white/70 transition-colors hover:bg-white/15 hover:text-white">
                <X size={14} />
              </button>
            </div>
            <div
              data-no-drag
              className="-mx-1 min-h-0 flex-1 cursor-default overflow-y-auto px-1 text-[12.5px]"
              style={{ scrollbarWidth: "thin", scrollbarColor: "rgb(255 255 255 / 0.3) transparent" }}
            >
              <PanelRow onClick={() => api.showMainWindow()}>Open Undertone</PanelRow>
              <PanelHeading>Position</PanelHeading>
              <div className="flex flex-wrap gap-1 px-1.5 pb-1">
                {(Object.keys(POSITION_LABELS) as IslandPosition[])
                  .filter((p) => p !== "custom" || o.position === "custom")
                  .map((p) => (
                    <Chip key={p} on={o.position === p} onClick={() => save({ position: p })}>
                      {POSITION_LABELS[p]}
                    </Chip>
                  ))}
              </div>
              <PanelHeading>Show</PanelHeading>
              {TOGGLES.map((t) => (
                <PanelRow key={t.key} on={!!o[t.key]} onClick={() => save({ [t.key]: !o[t.key] })}>
                  {t.text}
                </PanelRow>
              ))}
              <PanelHeading>Colours</PanelHeading>
              <div className="flex flex-wrap gap-1 px-1.5 pb-1">
                <Chip on={o.colors === "cover"} onClick={() => save({ colors: "cover" })}>
                  From the cover
                </Chip>
                {ISLAND_PALETTES.map((p) => (
                  <Chip key={p.id} on={o.colors === p.id} onClick={() => save({ colors: p.id })}>
                    {p.label}
                  </Chip>
                ))}
                <Chip on={o.colors === "custom"} onClick={() => save({ colors: "custom" })}>
                  My colours
                </Chip>
              </div>
              <div className="my-1.5 h-px bg-white/15" />
              <PanelRow
                onClick={() => {
                  closePicker();
                  api.showMainWindow();
                  emitTo("main", "ui:island-settings").catch(() => {});
                }}
              >
                More island settings…
              </PanelRow>
              <PanelRow onClick={() => api.updateSettings({ island: false }).catch(() => {})}>Hide the island</PanelRow>
            </div>
          </div>
        )}

        {/* Playlist picker */}
        {panel !== "options" && (
        <div className="absolute inset-0 flex flex-col px-3 pb-3 pt-3.5 transition-opacity duration-300" style={layer("list")}>
          <div className="flex items-center gap-2 px-1.5 pb-2.5">
            <FolderOpen size={15} className="text-white/80" />
            <span className="flex-1 text-[13.5px] font-semibold drop-shadow">Playlists</span>
            <span className="text-[11px] text-white/60">{shuffle ? "Plays shuffled" : "Plays in order"}</span>
            <button
              aria-label="Close"
              title="Close"
              onClick={closePicker}
              className="ml-1 rounded-full p-1 text-white/70 transition-colors hover:bg-white/15 hover:text-white"
            >
              <X size={14} />
            </button>
          </div>
          <div
            data-no-drag
            className="-mx-1 min-h-0 flex-1 cursor-default overflow-y-auto px-1"
            style={{ scrollbarWidth: "thin", scrollbarColor: "rgb(255 255 255 / 0.3) transparent" }}
          >
            {lists.length > 0 ? (
              lists.map((p) => (
                <div key={p.id} className="group flex items-center gap-1 rounded-xl transition-colors hover:bg-white/15">
                  <button onClick={() => pick(p.id, shuffle)} className="flex min-w-0 flex-1 items-center gap-3 rounded-xl p-1.5 text-left">
                    <Cover coverId={p.coverId ?? p.previewCovers[0] ?? null} className="h-10 w-10 shrink-0 shadow-md" rounded="rounded-lg" iconSize={16} />
                    <span className="min-w-0">
                      <span className="block truncate text-[13px] font-semibold drop-shadow">{p.name}</span>
                      <span className="block text-[11.5px] text-white/65">
                        {p.trackCount} song{p.trackCount === 1 ? "" : "s"}
                      </span>
                    </span>
                  </button>
                  <button
                    aria-label={`Shuffle ${p.name}`}
                    title="Shuffle"
                    onClick={() => pick(p.id, true)}
                    className="mr-1 rounded-full p-2 text-white/80 opacity-0 transition hover:bg-white/15 hover:text-white group-hover:opacity-100"
                  >
                    <Shuffle size={14} />
                  </button>
                  <button
                    aria-label={`Play ${p.name}`}
                    title="Play"
                    onClick={() => pick(p.id, false)}
                    className="mr-1.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-white text-black opacity-0 shadow transition hover:scale-105 group-hover:opacity-100"
                  >
                    <Play size={14} fill="currentColor" strokeWidth={0} className="translate-x-px" />
                  </button>
                </div>
              ))
            ) : (
              <div className="flex h-full items-center justify-center text-[12.5px] text-white/70">No playlists yet</div>
            )}
          </div>
        </div>
        )}

        {/* Full island */}
        <div className="absolute inset-0 flex flex-col gap-2.5 p-3.5 transition-opacity duration-300" style={layer("big")}>
          <div className="flex min-w-0 items-center gap-3">
            <button onClick={() => api.showMainWindow()} title="Open Undertone" className="shrink-0 transition-transform hover:scale-105">
              <Cover coverId={track.coverId} className="h-14 w-14 shadow-lg" rounded="rounded-xl" iconSize={20} />
            </button>
            <div className="min-w-0 flex-1">
              {showTitle && <div className="truncate text-[14.5px] font-semibold drop-shadow">{track.title}</div>}
              {showArtist && <div className="truncate text-[12.5px] text-white/75">{displayArtist(track)}</div>}
              <div className="mt-0.5 h-4 truncate text-[12px] italic text-white/90">{line ?? ""}</div>
            </div>
            <Bars levels={levels} big />
          </div>
          <div className="flex items-center gap-2 text-[10.5px] tabular text-white/80">
            <span className="w-8 text-right">{fmtTime(pos)}</span>
            <div data-no-drag className="relative h-1.5 flex-1 cursor-pointer overflow-hidden rounded-full bg-white/25" onClick={seek}>
              <div className="absolute inset-y-0 left-0 rounded-full bg-white" style={{ width: `${fraction * 100}%` }} />
            </div>
            <span className="w-8">-{fmtTime(Math.max(0, duration - pos))}</span>
          </div>
          <div className="flex items-center justify-center gap-4">
            <Btn label={shuffle ? "Shuffle: on" : "Shuffle: off"} active={shuffle} onClick={() => api.setShuffle(!shuffle)}>
              <Shuffle size={16} />
            </Btn>
            <Btn label="Previous" onClick={() => api.previous()}>
              <SkipBack size={18} fill="currentColor" />
            </Btn>
            <button
              aria-label={playing ? "Pause" : "Play"}
              onClick={() => api.togglePlay()}
              className="flex h-10 w-10 items-center justify-center rounded-full bg-white text-black shadow-md transition-transform hover:scale-105 active:scale-95"
            >
              {playing ? <Pause size={18} fill="currentColor" strokeWidth={0} /> : <Play size={18} fill="currentColor" strokeWidth={0} className="translate-x-px" />}
            </button>
            <Btn label="Next" onClick={() => api.next()}>
              <SkipForward size={18} fill="currentColor" />
            </Btn>
            <Btn label={repeatLabel} active={repeat !== "off"} onClick={() => api.setRepeat(NEXT_REPEAT[repeat])}>
              <RepeatIcon size={16} />
            </Btn>
            {o.showPlaylists && (
              <Btn label="Play a playlist" onClick={openPicker}>
                <FolderOpen size={16} />
              </Btn>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function PanelHeading({ children }: { children: ReactNode }) {
  return <div className="px-1.5 pb-1 pt-2 text-[10.5px] font-semibold uppercase tracking-wider text-white/55">{children}</div>;
}

/** A row in the options panel; with `on`, a yes/no option with a tick. */
function PanelRow({ on, onClick, children }: { on?: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      role={on === undefined ? undefined : "menuitemcheckbox"}
      aria-checked={on}
      onClick={onClick}
      className="flex w-full items-center gap-2 rounded-lg px-1.5 py-1 text-left transition-colors hover:bg-white/15"
    >
      {on !== undefined && (
        <span className={`flex h-3.5 w-3.5 shrink-0 items-center justify-center rounded ${on ? "bg-white text-black" : "ring-1 ring-white/40"}`}>
          {on && <Check size={10} strokeWidth={3} />}
        </span>
      )}
      <span className="min-w-0 truncate drop-shadow">{children}</span>
    </button>
  );
}

function Chip({ on, onClick, children }: { on: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      aria-pressed={on}
      onClick={onClick}
      className={`rounded-full px-2.5 py-0.5 text-[11.5px] transition-colors ${on ? "bg-white font-semibold text-black" : "bg-white/10 text-white/85 hover:bg-white/20"}`}
    >
      {children}
    </button>
  );
}

function TitleLine({ title, artist }: { title: string | null; artist: string | null }) {
  return (
    <div className="min-w-0 flex-1 truncate text-[12.5px] font-semibold drop-shadow">
      {title}
      {artist && (
        <span className="font-normal text-white/70">
          {title ? " · " : ""}
          {artist}
        </span>
      )}
    </div>
  );
}

/** Shuffle and repeat are dimmed while off. */
function SmallBtn({ label, onClick, active, children }: { label: string; onClick: (e: MouseEvent) => void; active?: boolean; children: ReactNode }) {
  return (
    <button
      aria-label={label}
      aria-pressed={active}
      title={label}
      onClick={onClick}
      className={`flex h-[26px] w-[26px] items-center justify-center rounded-full transition-colors hover:bg-white/15 hover:text-white ${
        active === false ? "text-white/45" : "text-white/90"
      }`}
    >
      {children}
    </button>
  );
}

function Btn({ label, onClick, active, children }: { label: string; onClick: (e: MouseEvent) => void; active?: boolean; children: ReactNode }) {
  return (
    <button
      aria-label={label}
      aria-pressed={active}
      title={label}
      onClick={onClick}
      className={`rounded-full p-2 transition-colors hover:bg-white/15 hover:text-white ${active === false ? "text-white/45" : "text-white/90"}`}
    >
      {children}
    </button>
  );
}

/**
 * Level bars that follow the music: the backend's band levels (low to high, 0..1). The small bars
 * show them in pairs, the big ones each.
 */
function Bars({ levels, big = false }: { levels: number[]; big?: boolean }) {
  const count = big ? 8 : 4;
  const per = Math.max(1, Math.floor(levels.length / count));
  const bars = Array.from({ length: count }, (_, i) => Math.max(0, ...levels.slice(i * per, i * per + per)));
  return (
    <div className={`flex shrink-0 items-center ${big ? "h-6 gap-[2.5px]" : "h-4 gap-[3px]"}`} aria-hidden>
      {bars.map((level, i) => (
        <span
          key={i}
          className="h-full w-[3px] rounded-full bg-white/90"
          style={{
            transformOrigin: "50% 50%",
            transform: `scaleY(${0.22 + 0.78 * level})`,
            transition: "transform 90ms linear",
          }}
        />
      ))}
    </div>
  );
}
