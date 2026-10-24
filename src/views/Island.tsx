import { emitTo, listen } from "@tauri-apps/api/event";
import { Menu } from "@tauri-apps/api/menu";
import { availableMonitors, currentMonitor, getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { Pause, Play, Repeat, Repeat1, Shuffle, SkipBack, SkipForward } from "lucide-react";
import { useEffect, useRef, useState, type MouseEvent, type ReactNode } from "react";
import { Cover } from "../components/Cover";
import { api } from "../lib/api";
import { displayArtist, fmtTime } from "../lib/format";
import {
  areaAt,
  collapsedFrom,
  collapsedRect,
  FALLBACK_PALETTE,
  fixedPalette,
  grownRect,
  ISLAND_PALETTES,
  POSITION_LABELS,
  type Rect,
} from "../lib/island";
import { coverPalette, type Palette } from "../lib/palette";
import type { IslandOptions, IslandPosition, RepeatMode } from "../lib/types";
import { useLyrics } from "../store/lyrics";
import { initPlayer, livePosition, usePlayer } from "../store/player";

type Size = { w: number; h: number };
/** The small bar, the card shown when a new song starts, and the full island on hover. */
type Level = "small" | "card" | "big";
const RANK: Record<Level, number> = { small: 0, card: 1, big: 2 };
/** Where the window goes at each size (physical px), from where the small bar sits. */
type Geo = { rects: Record<Level, Rect>; sf: number; up: boolean };

/** Window sizes (logical px). The pill sits inside with room for its glow. */
const CARD = { w: 400, h: 98 };
const BIG = { w: 500, h: 186 };
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
  return { w: 68 + (text ? 220 : 0) + controls, h: 58 };
}

function levelSize(level: Level, small: Size): Size {
  return level === "big" ? BIG : level === "card" ? CARD : small;
}

/** The pill's size at each level. */
function pillSize(level: Level, small: Size): Size {
  const s = levelSize(level, small);
  return { w: s.w - 2 * SIDE, h: level === "small" ? s.h - 18 : s.h - 22 };
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
async function layout(o: IslandOptions, small: Size): Promise<Geo | null> {
  const monitors = await availableMonitors();
  const areas = monitors.map((m) => ({
    rect: { x: m.workArea.position.x, y: m.workArea.position.y, w: m.workArea.size.width, h: m.workArea.size.height } as Rect,
    sf: m.scaleFactor,
  }));
  let area: { rect: Rect; sf: number } | undefined;
  if (o.position === "custom" && o.x != null && o.y != null) {
    const r = areaAt(areas.map((a) => a.rect), o.x, o.y);
    area = areas.find((a) => a.rect === r);
  } else {
    // Corners and edges of the screen the island is on now.
    const m = await currentMonitor();
    area = areas.find((a) => m && a.rect.x === m.workArea.position.x && a.rect.y === m.workArea.position.y) ?? areas[0];
  }
  if (!area) return null;
  const px = (s: Size) => ({ w: Math.round(s.w * area.sf), h: Math.round(s.h * area.sf) });
  const collapsed = collapsedRect(o, area.rect, px(small));
  const card = grownRect(collapsed, px(CARD), area.rect);
  const big = grownRect(collapsed, px(BIG), area.rect);
  return { rects: { small: collapsed, card: card.rect, big: big.rect }, sf: area.sf, up: big.up };
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

/** The right-click menu: where the island sits, what it shows, and its colours. */
async function showMenu() {
  const o = options();
  if (!o) return;
  const check = (text: string, checked: boolean, patch: Partial<IslandOptions>) => ({ text, checked, action: () => save(patch) });
  const positions = (Object.keys(POSITION_LABELS) as IslandPosition[])
    .filter((p) => p !== "custom" || o.position === "custom")
    .map((p) => check(POSITION_LABELS[p], o.position === p, { position: p }));
  const colors = [
    check("From the cover", o.colors === "cover", { colors: "cover" }),
    ...ISLAND_PALETTES.map((p) => check(p.label, o.colors === p.id, { colors: p.id })),
    check("My colours", o.colors === "custom", { colors: "custom" }),
  ];
  const menu = await Menu.new({
    items: [
      { text: "Open Undertone", action: () => api.showMainWindow() },
      { item: "Separator" },
      { text: "Position", items: positions },
      check("Lock position", o.locked, { locked: !o.locked }),
      check("Let clicks through (hold Alt to use it)", o.clickThrough, { clickThrough: !o.clickThrough }),
      { item: "Separator" },
      check("Show the song title", o.showTitle, { showTitle: !o.showTitle }),
      check("Show the artist", o.showArtist, { showArtist: !o.showArtist }),
      check("Show play controls", o.showControls, { showControls: !o.showControls }),
      check("Show shuffle and repeat", o.showShuffleRepeat, { showShuffleRepeat: !o.showShuffleRepeat }),
      check("Show the seek bar", o.showProgress, { showProgress: !o.showProgress }),
      check("Pop up when a song starts", o.peekOnNewSong, { peekOnNewSong: !o.peekOnNewSong }),
      check("Open when pointed at", o.expandOnHover, { expandOnHover: !o.expandOnHover }),
      check("Only while Undertone is minimised", o.onlyWhenMinimized, { onlyWhenMinimized: !o.onlyWhenMinimized }),
      check("Hide over full-screen games and videos", o.hideInFullscreen, { hideInFullscreen: !o.hideInFullscreen }),
      { text: "Colours", items: colors },
      { item: "Separator" },
      {
        text: "More island settings…",
        action: () => {
          api.showMainWindow();
          emitTo("main", "ui:island-settings").catch(() => {});
        },
      },
      { text: "Hide the island", action: () => api.updateSettings({ island: false }).catch(() => {}) },
    ],
  });
  await menu.popup();
}

/** Opens the menu. Letting go of Alt closes menus, so after Alt + right-click wait for that first. */
async function openMenu(altHeld: boolean) {
  if (altHeld) {
    const until = Date.now() + 5000;
    while (Date.now() < until && (await api.islandAltDown())) await new Promise((r) => setTimeout(r, 40));
  }
  await showMenu();
}

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
  const [mainAway, setMainAway] = useState(false);
  const [hover, setHover] = useState(false);
  const [peek, setPeek] = useState(false);
  /** The size the window should be, the size it is, and the pill's (which follows it). */
  const [winLevel, setWinLevel] = useState<Level>("small");
  const [placed, setPlaced] = useState<Level>("small");
  const [pill, setPill] = useState<Level>("small");
  const [geo, setGeo] = useState<Geo | null>(null);
  const [palettes, setPalettes] = useState<{ key: string; p: Palette }[]>([{ key: "none", p: FALLBACK_PALETTE }]);
  const [pos, setPos] = useState(0);
  const leave = useRef<number>();
  const lastPath = useRef<string | null>(null);
  const press = useRef<{ x: number; y: number } | null>(null);
  const dragging = useRef(false);
  const upRef = useRef(false);
  const targetRef = useRef<Level>("small");
  const line = useLyricLine(track?.path ?? null);

  const showTitle = o?.showTitle ?? true;
  const showArtist = o?.showArtist ?? true;
  const small = o ? smallSize(o) : { w: 320, h: 58 };
  const visible = !!o && !!track && !(fullscreen && o.hideInFullscreen) && (!o.onlyWhenMinimized || mainAway);
  const target: Level = !visible ? "small" : hover && o?.expandOnHover ? "big" : peek && o?.peekOnNewSong ? "card" : "small";
  targetRef.current = target;
  const placeKey = o ? `${o.position}|${o.x}|${o.y}|${small.w}` : "";

  useEffect(() => {
    for (const el of [document.documentElement, document.body]) el.style.background = "transparent";
    initPlayer().catch((e) => console.error(e));
    api.islandMainAway().then(setMainAway).catch(() => {});
    const uns = [
      listen<boolean>("island:fullscreen", (e) => setFullscreen(e.payload)),
      listen<boolean>("island:main-away", (e) => setMainAway(e.payload)),
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
        const cur = options();
        if (!cur) return;
        const [p, size, sf] = await Promise.all([win.outerPosition(), win.outerSize(), win.scaleFactor()]);
        const s = smallSize(cur);
        const spot = collapsedFrom(p, { w: size.width, h: size.height }, { w: s.w * sf, h: s.h * sf }, upRef.current);
        save({ position: "custom", ...spot });
      }, 350);
    });
    return () => {
      window.clearTimeout(settle);
      for (const un of [...uns, unMoved]) un.then((f) => f());
    };
  }, []);

  // Growing: enlarge the window first, then the pill. Shrinking: the pill first, then the window.
  useEffect(() => {
    if (RANK[target] >= RANK[winLevel]) {
      setWinLevel(target);
      return;
    }
    setPill(target);
    const t = window.setTimeout(() => setWinLevel(target), 380);
    return () => window.clearTimeout(t);
  }, [target]); // eslint-disable-line react-hooks/exhaustive-deps

  // Place and show the window, or hide it.
  useEffect(() => {
    if (!visible || !o) {
      win.hide();
      return;
    }
    let alive = true;
    (async () => {
      const g = await layout(o, small);
      if (!alive) return;
      if (g) {
        const r = g.rects[winLevel];
        await api.islandSetBounds(Math.round(r.x), Math.round(r.y), Math.round(r.w), Math.round(r.h));
      } else {
        const s = levelSize(winLevel, small);
        await win.setSize(new LogicalSize(s.w, s.h));
      }
      if (!alive) return;
      upRef.current = g?.up ?? false;
      setGeo(g);
      setPlaced(winLevel);
      await win.show();
      if (targetRef.current === winLevel) requestAnimationFrame(() => setPill(winLevel));
    })().catch((e) => console.error(e));
    return () => {
      alive = false;
    };
  }, [visible, winLevel, placeKey]); // eslint-disable-line react-hooks/exhaustive-deps

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
    const id = window.setInterval(tick, pill === "big" ? 250 : 500);
    return () => window.clearInterval(id);
  }, [visible, pill, state]);

  if (!track || !o) return null;
  const playing = state === "playing";
  const glow = palettes[palettes.length - 1].p[0];
  const onControl = (e: MouseEvent) => !!(e.target as HTMLElement).closest("button, [data-no-drag]");
  const fraction = duration > 0 ? Math.min(1, pos / duration) : 0;
  const seek = (e: MouseEvent<HTMLElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    if (duration > 0) api.seek(Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)) * duration);
  };
  // The pill sits where the window would be at its own size, inside the window as it is now; so
  // it grows from and shrinks back to exactly where the small bar sits.
  const box = pillSize(pill, small);
  const own = levelSize(pill, small);
  const dx = geo ? (geo.rects[pill].x - geo.rects[placed].x) / geo.sf : (levelSize(placed, small).w - own.w) / 2;
  const dy = geo ? (geo.rects[pill].y - geo.rects[placed].y) / geo.sf : 0;
  const up = geo?.up ?? false;
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
        @keyframes island-bar { 0%,100% { transform: scaleY(.35); } 50% { transform: scaleY(1); } }
      `}</style>
      <div
        onMouseEnter={() => {
          window.clearTimeout(leave.current);
          setHover(true);
        }}
        onMouseLeave={() => {
          press.current = null;
          leave.current = window.setTimeout(() => setHover(false), 350);
        }}
        onMouseDown={(e) => {
          press.current = e.button === 0 && !o.locked && !onControl(e) ? { x: e.screenX, y: e.screenY } : null;
        }}
        onMouseMove={(e) => {
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
          openMenu(e.altKey).catch((err) => console.error(err));
        }}
        className={`absolute select-none overflow-hidden text-white ${o.locked ? "" : "cursor-grab active:cursor-grabbing"}`}
        style={{
          left: dx + SIDE,
          top: dy + (up ? own.h - box.h - EDGE : EDGE),
          width: box.w,
          height: box.h,
          borderRadius: pill === "big" ? 28 : pill === "card" ? 24 : 22,
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
              animation: `island-fade 700ms ease-out both${playing ? ", island-drift 14s ease-in-out infinite" : ""}`,
            }}
          />
        ))}
        <div className="absolute inset-0 bg-black/25" />

        {/* Small bar */}
        <div className="absolute inset-0 transition-opacity duration-200" style={layer("small")}>
          <div className="flex h-full items-center gap-2.5 px-2.5">
            <Cover coverId={track.coverId} className="h-7 w-7 shrink-0" rounded="rounded-full" iconSize={12} />
            {showTitle || showArtist ? <TitleLine title={showTitle ? track.title : null} artist={showArtist ? displayArtist(track) : null} /> : null}
            {o.showControls ? (
              <div className="ml-auto flex shrink-0 items-center gap-0.5">
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
              </div>
            ) : (
              <div className="ml-auto">
                <Bars playing={playing} />
              </div>
            )}
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
          <Bars playing={playing} />
        </div>

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
            <Bars playing={playing} big />
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
          </div>
        </div>
      </div>
    </div>
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
function SmallBtn({ label, onClick, active, children }: { label: string; onClick: () => void; active?: boolean; children: ReactNode }) {
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

function Btn({ label, onClick, active, children }: { label: string; onClick: () => void; active?: boolean; children: ReactNode }) {
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

/** Little level bars that move while the song plays. */
function Bars({ playing, big = false }: { playing: boolean; big?: boolean }) {
  return (
    <div className={`flex shrink-0 items-center gap-[3px] ${big ? "h-6" : "h-4"}`} aria-hidden>
      {[0.9, 0.6, 1.1, 0.75].map((d, i) => (
        <span
          key={i}
          className={`h-full rounded-full bg-white/90 ${big ? "w-[3.5px]" : "w-[3px]"}`}
          style={{
            transformOrigin: "50% 50%",
            transform: "scaleY(.35)",
            animation: playing ? `island-bar ${d}s ease-in-out ${i * 0.13}s infinite` : "none",
          }}
        />
      ))}
    </div>
  );
}
