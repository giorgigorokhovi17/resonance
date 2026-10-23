import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, LogicalPosition, LogicalSize, primaryMonitor } from "@tauri-apps/api/window";
import { Pause, Play, SkipBack, SkipForward } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { Cover } from "../components/Cover";
import { api } from "../lib/api";
import { displayArtist, fmtTime } from "../lib/format";
import { coverPalette, type Palette } from "../lib/palette";
import { useLyrics } from "../store/lyrics";
import { initPlayer, livePosition, usePlayer } from "../store/player";

/** Window sizes (logical px). The pill sits inside with room for its glow. */
const SMALL = { w: 320, h: 58 };
const BIG = { w: 500, h: 186 };
/** How long the island stays open after a new song starts. */
const PEEK_MS = 4000;
const FALLBACK: Palette = ["hsl(250 30% 30%)", "hsl(280 30% 26%)", "hsl(220 30% 22%)"];

async function place(size: { w: number; h: number }) {
  const win = getCurrentWindow();
  const m = await primaryMonitor();
  await win.setSize(new LogicalSize(size.w, size.h));
  if (!m) return;
  const sf = m.scaleFactor;
  const x = m.workArea.position.x / sf + (m.workArea.size.width / sf - size.w) / 2;
  await win.setPosition(new LogicalPosition(Math.round(x), Math.round(m.workArea.position.y / sf + 6)));
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

/**
 * The Dynamic Island: a pill at the top of the screen with the playing song, coloured from its
 * cover. It grows on hover (and briefly when a song starts) into controls, progress and the
 * current lyric line.
 */
export function Island() {
  const track = usePlayer((s) => s.status?.track ?? null);
  const state = usePlayer((s) => s.status?.state ?? "stopped");
  const duration = usePlayer((s) => s.status?.duration ?? 0);
  const [fullscreen, setFullscreen] = useState(false);
  const [hover, setHover] = useState(false);
  const [peek, setPeek] = useState(false);
  const [big, setBig] = useState(false);
  const [open, setOpen] = useState(false);
  const [palettes, setPalettes] = useState<{ key: string; p: Palette }[]>([{ key: "none", p: FALLBACK }]);
  const [pos, setPos] = useState(0);
  const leave = useRef<number>();
  const lastPath = useRef<string | null>(null);
  const line = useLyricLine(track?.path ?? null);
  const visible = !!track && state !== "stopped" && !fullscreen;
  const want = visible && (hover || peek);

  useEffect(() => {
    for (const el of [document.documentElement, document.body]) el.style.background = "transparent";
    initPlayer().catch((e) => console.error(e));
    const un = listen<boolean>("island:fullscreen", (e) => setFullscreen(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);

  // Show or hide the window with the song.
  useEffect(() => {
    const win = getCurrentWindow();
    if (visible) place(big ? BIG : SMALL).then(() => win.show());
    else win.hide();
  }, [visible]); // eslint-disable-line react-hooks/exhaustive-deps

  // Growing: enlarge the window first, then the pill. Shrinking: the pill first, then the window.
  useEffect(() => {
    let t: number | undefined;
    if (want) {
      setBig(true);
      place(BIG).then(() => requestAnimationFrame(() => setOpen(true)));
    } else {
      setOpen(false);
      t = window.setTimeout(() => {
        setBig(false);
        place(SMALL);
      }, 380);
    }
    return () => window.clearTimeout(t);
  }, [want]);

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

  // Colours from the cover; the new gradient fades in over the old one.
  useEffect(() => {
    const id = track?.coverId;
    if (!id) {
      setPalettes((ps) => [...ps.slice(-1), { key: `none-${Date.now()}`, p: FALLBACK }]);
      return;
    }
    let alive = true;
    coverPalette(id).then((p) => alive && setPalettes((ps) => [...ps.slice(-1), { key: id, p: p ?? FALLBACK }]));
    return () => {
      alive = false;
    };
  }, [track?.coverId]);

  useEffect(() => {
    if (!open) return;
    const tick = () => setPos(livePosition());
    tick();
    const id = window.setInterval(tick, 250);
    return () => window.clearInterval(id);
  }, [open]);

  if (!track) return null;
  const playing = state === "playing";
  const glow = palettes[palettes.length - 1].p[0];

  return (
    <div className="flex h-full w-full justify-center pt-1.5">
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
          leave.current = window.setTimeout(() => setHover(false), 350);
        }}
        className="relative overflow-hidden text-white"
        style={{
          width: open ? BIG.w - 20 : SMALL.w - 20,
          height: open ? BIG.h - 22 : SMALL.h - 18,
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
          <div className="min-w-0 flex-1 truncate text-[12.5px] font-semibold drop-shadow">
            {track.title}
            <span className="font-normal text-white/70"> · {displayArtist(track)}</span>
          </div>
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
              <div className="truncate text-[14.5px] font-semibold drop-shadow">{track.title}</div>
              <div className="truncate text-[12.5px] text-white/75">{displayArtist(track)}</div>
              <div className="mt-0.5 h-4 truncate text-[12px] italic text-white/90">{line ?? ""}</div>
            </div>
            <Bars playing={playing} big />
          </div>
          <div className="flex items-center gap-2 text-[10.5px] tabular text-white/80">
            <span className="w-8 text-right">{fmtTime(pos)}</span>
            <div
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
