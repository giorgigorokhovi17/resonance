import { emitTo, listen } from "@tauri-apps/api/event";
import { Menu } from "@tauri-apps/api/menu";
import { availableMonitors, currentMonitor, getCurrentWindow, LogicalSize, PhysicalPosition } from "@tauri-apps/api/window";
import { Pause, Play, SkipBack, SkipForward } from "lucide-react";
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
import type { IslandOptions, IslandPosition } from "../lib/types";
import { useLyrics } from "../store/lyrics";
import { initPlayer, livePosition, usePlayer } from "../store/player";

/** Window sizes (logical px). The pill sits inside with room for its glow. */
const SMALL = { w: 320, h: 58 };
/** Collapsed with neither the title nor the artist: just the cover and the level bars. */
const TINY = { w: 124, h: 58 };
const BIG = { w: 500, h: 186 };
/** How long the island stays open after a new song starts. */
const PEEK_MS = 4000;
/** The mouse moves this far (px) with the button down before a press becomes a drag. */
const DRAG_START = 4;

const win = getCurrentWindow();

function options(): IslandOptions | null {
  return usePlayer.getState().settings?.islandOptions ?? null;
}

function save(patch: Partial<IslandOptions>) {
  const o = options();
  if (o) api.updateSettings({ islandOptions: { ...o, ...patch } }).catch((e) => console.error(e));
}

/**
 * Sizes and moves the window for the options: collapsed (`small`) or opened (BIG). Works in
 * physical pixels so it lands right on screens with different scaling. Returns whether the
 * opened island grew upwards.
 */
async function place(o: IslandOptions, small: { w: number; h: number }, big: boolean): Promise<boolean> {
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
  const size = big ? BIG : small;
  await win.setSize(new LogicalSize(size.w, size.h));
  if (!area) return false;
  const px = (s: { w: number; h: number }) => ({ w: s.w * area.sf, h: s.h * area.sf });
  const collapsed = collapsedRect(o, area.rect, px(small));
  const { rect, up } = big ? grownRect(collapsed, px(BIG), area.rect) : { rect: collapsed, up: false };
  await win.setPosition(new PhysicalPosition(Math.round(rect.x), Math.round(rect.y)));
  return up;
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
      check("Let clicks through (undo in the tray menu)", o.clickThrough, { clickThrough: !o.clickThrough }),
      { item: "Separator" },
      check("Show the song title", o.showTitle, { showTitle: !o.showTitle }),
      check("Show the artist", o.showArtist, { showArtist: !o.showArtist }),
      check("Open when pointed at", o.expandOnHover, { expandOnHover: !o.expandOnHover }),
      check("Only while Undertone is minimised", o.onlyWhenMinimized, { onlyWhenMinimized: !o.onlyWhenMinimized }),
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

/**
 * The Dynamic Island: a pill with the playing song, coloured from its cover or a chosen palette.
 * It grows on hover (and briefly when a song starts) into controls, progress and the current
 * lyric line. Drag it anywhere, right-click it for options, double-click it to open Undertone.
 */
export function Island() {
  const track = usePlayer((s) => s.status?.track ?? null);
  const state = usePlayer((s) => s.status?.state ?? "stopped");
  const duration = usePlayer((s) => s.status?.duration ?? 0);
  const o = usePlayer((s) => s.settings?.islandOptions ?? null);
  const [fullscreen, setFullscreen] = useState(false);
  const [mainAway, setMainAway] = useState(false);
  const [hover, setHover] = useState(false);
  const [peek, setPeek] = useState(false);
  const [big, setBig] = useState(false);
  const [open, setOpen] = useState(false);
  const [up, setUp] = useState(false);
  const [palettes, setPalettes] = useState<{ key: string; p: Palette }[]>([{ key: "none", p: FALLBACK_PALETTE }]);
  const [pos, setPos] = useState(0);
  const leave = useRef<number>();
  const lastPath = useRef<string | null>(null);
  const press = useRef<{ x: number; y: number } | null>(null);
  const dragging = useRef(false);
  const upRef = useRef(false);
  const wantRef = useRef(false);
  const line = useLyricLine(track?.path ?? null);

  const showTitle = o?.showTitle ?? true;
  const showArtist = o?.showArtist ?? true;
  const small = showTitle || showArtist ? SMALL : TINY;
  const visible = !!o && !!track && state !== "stopped" && !fullscreen && (!o.onlyWhenMinimized || mainAway);
  const want = visible && ((hover && (o?.expandOnHover ?? true)) || peek);
  wantRef.current = want;
  const placeKey = o ? `${o.position}|${o.x}|${o.y}|${small.w}` : "";

  useEffect(() => {
    for (const el of [document.documentElement, document.body]) el.style.background = "transparent";
    initPlayer().catch((e) => console.error(e));
    api.islandMainAway().then(setMainAway).catch(() => {});
    const uns = [
      listen<boolean>("island:fullscreen", (e) => setFullscreen(e.payload)),
      listen<boolean>("island:main-away", (e) => setMainAway(e.payload)),
    ];
    // A drag ends when the window stops moving: keep where it was dropped.
    let settle: number | undefined;
    const unMoved = win.onMoved(() => {
      if (!dragging.current) return;
      window.clearTimeout(settle);
      settle = window.setTimeout(async () => {
        dragging.current = false;
        const [p, size, sf] = await Promise.all([win.outerPosition(), win.outerSize(), win.scaleFactor()]);
        const cur = options();
        const s = cur && (cur.showTitle || cur.showArtist) ? SMALL : TINY;
        const spot = collapsedFrom(p, { w: size.width, h: size.height }, { w: s.w * sf, h: s.h * sf }, upRef.current);
        save({ position: "custom", ...spot });
      }, 350);
    });
    return () => {
      window.clearTimeout(settle);
      for (const un of [...uns, unMoved]) un.then((f) => f());
    };
  }, []);

  useEffect(() => {
    win.setIgnoreCursorEvents(o?.clickThrough ?? false).catch(() => {});
    if (o?.clickThrough) setHover(false);
  }, [o?.clickThrough]);

  // Growing: enlarge the window first, then the pill. Shrinking: the pill first, then the window.
  useEffect(() => {
    let t: number | undefined;
    if (want) setBig(true);
    else {
      setOpen(false);
      t = window.setTimeout(() => setBig(false), 380);
    }
    return () => window.clearTimeout(t);
  }, [want]);

  // Place and show the window, or hide it.
  useEffect(() => {
    if (!visible || !o) {
      win.hide();
      return;
    }
    let alive = true;
    place(o, small, big)
      .then((grewUp) => {
        if (!alive) return;
        upRef.current = grewUp;
        setUp(grewUp);
        win.show();
        if (big && wantRef.current) requestAnimationFrame(() => setOpen(true));
      })
      .catch((e) => console.error(e));
    return () => {
      alive = false;
    };
  }, [visible, big, placeKey]); // eslint-disable-line react-hooks/exhaustive-deps

  // A new song opens the island for a moment.
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
    if (!open) return;
    const tick = () => setPos(livePosition());
    tick();
    const id = window.setInterval(tick, 250);
    return () => window.clearInterval(id);
  }, [open]);

  if (!track || !o) return null;
  const playing = state === "playing";
  const glow = palettes[palettes.length - 1].p[0];
  const onControl = (e: MouseEvent) => !!(e.target as HTMLElement).closest("button, [data-no-drag]");

  return (
    <div className={`flex h-full w-full justify-center ${up ? "items-end pb-1.5" : "items-start pt-1.5"}`}>
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
          showMenu().catch((err) => console.error(err));
        }}
        className={`relative select-none overflow-hidden text-white ${o.locked ? "" : "cursor-grab active:cursor-grabbing"}`}
        style={{
          width: open ? BIG.w - 20 : small.w - 20,
          height: open ? BIG.h - 22 : small.h - 18,
          borderRadius: open ? 28 : 22,
          boxShadow: `0 10px 30px -10px ${glow}, 0 0 0 1px rgb(255 255 255 / 0.08) inset`,
          transition: "width 380ms cubic-bezier(.2,.9,.25,1.15), height 380ms cubic-bezier(.2,.9,.25,1.15), border-radius 380ms, box-shadow 600ms",
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

        {/* Collapsed */}
        <div
          className="absolute inset-0 flex items-center gap-2.5 px-2.5 transition-opacity duration-200"
          style={{ opacity: open ? 0 : 1, pointerEvents: open ? "none" : "auto" }}
        >
          <Cover coverId={track.coverId} className="h-7 w-7 shrink-0" rounded="rounded-full" iconSize={12} />
          {showTitle || showArtist ? (
            <div className="min-w-0 flex-1 truncate text-[12.5px] font-semibold drop-shadow">
              {showTitle && track.title}
              {showArtist && (
                <span className="font-normal text-white/70">
                  {showTitle ? " · " : ""}
                  {displayArtist(track)}
                </span>
              )}
            </div>
          ) : (
            <div className="flex-1" />
          )}
          <Bars playing={playing} />
        </div>

        {/* Expanded */}
        <div
          className="absolute inset-0 flex flex-col gap-2.5 p-3.5 transition-opacity duration-300"
          style={{ opacity: open ? 1 : 0, transitionDelay: open ? "120ms" : "0ms", pointerEvents: open ? "auto" : "none" }}
        >
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
            <div
              data-no-drag
              className="relative h-1.5 flex-1 cursor-pointer overflow-hidden rounded-full bg-white/25"
              onClick={(e) => {
                const r = e.currentTarget.getBoundingClientRect();
                if (duration > 0) api.seek(((e.clientX - r.left) / r.width) * duration);
              }}
            >
              <div className="absolute inset-y-0 left-0 rounded-full bg-white" style={{ width: `${duration ? Math.min(100, (pos / duration) * 100) : 0}%` }} />
            </div>
            <span className="w-8">-{fmtTime(Math.max(0, duration - pos))}</span>
          </div>
          <div className="flex items-center justify-center gap-5">
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
          </div>
        </div>
      </div>
    </div>
  );
}

function Btn({ label, onClick, children }: { label: string; onClick: () => void; children: ReactNode }) {
  return (
    <button aria-label={label} title={label} onClick={onClick} className="rounded-full p-2 text-white/90 transition-colors hover:bg-white/15 hover:text-white">
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
