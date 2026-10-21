import { Mic2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api } from "../lib/api";
import { displayArtist } from "../lib/format";
import type { Lyrics } from "../lib/types";
import { useLiveCover, useLiveMotion } from "../store/library";
import { livePosition, usePlayer } from "../store/player";
import { Cover } from "./Cover";

/** Lyrics by file path; `null` = the song has none. */
const cache = new Map<string, Lyrics | null>();

/**
 * The playing song and its lyrics, from a `.lrc` file next to it or its tags. Timed lyrics follow
 * the song (click a line to jump there); plain lyrics are shown as written.
 */
export function LyricsPanel({ className = "" }: { className?: string }) {
  const track = usePlayer((s) => s.status?.track ?? s.queue.items.find((i) => i.uid === s.queue.currentUid)?.track ?? null);
  const coverId = useLiveCover(track);
  const motionId = useLiveMotion(track);
  const path = track?.path ?? null;
  const [lyrics, setLyrics] = useState<Lyrics | null | undefined>(() => (path ? cache.get(path) : null));
  const [current, setCurrent] = useState(-1);
  const scroller = useRef<HTMLDivElement>(null);
  const userScrolled = useRef(0);

  useEffect(() => {
    let alive = true;
    if (!path) {
      setLyrics(null);
      return;
    }
    if (cache.has(path)) {
      setLyrics(cache.get(path));
    } else {
      setLyrics(undefined);
      api
        .getLyrics(path)
        .catch(() => null)
        .then((l) => {
          cache.set(path, l);
          if (alive) setLyrics(l);
        });
    }
    scroller.current?.scrollTo({ top: 0 });
    return () => {
      alive = false;
    };
  }, [path]);

  // The line being sung, for timed lyrics.
  useEffect(() => {
    setCurrent(-1);
    if (!lyrics?.synced) return;
    const lines = lyrics.lines;
    const tick = () => {
      const ms = livePosition() * 1000;
      let i = -1;
      while (i + 1 < lines.length && (lines[i + 1].timeMs ?? 0) <= ms) i++;
      setCurrent((c) => (c === i ? c : i));
    };
    tick();
    const id = window.setInterval(tick, 200);
    return () => window.clearInterval(id);
  }, [lyrics]);

  // Keep the current line in the middle, unless the user just scrolled the lyrics themselves.
  useEffect(() => {
    const box = scroller.current;
    const line = box?.querySelector<HTMLElement>(`[data-line="${current}"]`);
    if (!box || !line || Date.now() - userScrolled.current < 4000) return;
    box.scrollTo({ top: line.offsetTop - box.clientHeight / 2 + line.clientHeight / 2, behavior: "smooth" });
  }, [current]);

  return (
    <section className={`flex min-h-0 flex-col overflow-hidden rounded-xl bg-elev/70 ring-1 ring-line ${className}`} aria-label="Lyrics">
      <div className="flex shrink-0 items-center gap-3 border-b border-line/70 p-3">
        <Cover coverId={coverId} motionId={motionId} motion="tile" className="h-11 w-11 shrink-0" rounded="rounded" iconSize={18} />
        <div className="min-w-0">
          <div className="text-[10.5px] font-semibold uppercase tracking-wider text-muted">{track ? "Now playing" : "Lyrics"}</div>
          <div className="truncate text-[13.5px] font-semibold">{track?.title ?? "Nothing playing"}</div>
          {track && <div className="truncate text-[12px] text-muted">{displayArtist(track)}</div>}
        </div>
      </div>
      <div
        ref={scroller}
        onWheel={() => (userScrolled.current = Date.now())}
        className="relative min-h-0 flex-1 overflow-y-auto px-4 py-3"
      >
        {!track ? (
          <Empty text="Play a song to see its lyrics here." />
        ) : lyrics === undefined ? null : !lyrics ? (
          <Empty text="No lyrics for this song. Undertone shows lyrics saved in the song's tags, or in a .lrc file with the same name next to it." />
        ) : lyrics.synced ? (
          <div className="space-y-1.5 py-[30%]">
            {lyrics.lines.map((l, i) => (
              <button
                key={i}
                data-line={i}
                onClick={() => api.seek((l.timeMs ?? 0) / 1000)}
                title="Play from here"
                className={`block w-full rounded px-1 text-left text-[15px] font-semibold leading-snug transition-colors duration-300 hover:text-fg ${
                  i === current ? "text-fg" : i < current ? "text-fg/35" : "text-fg/55"
                }`}
              >
                {l.text || "♪"}
              </button>
            ))}
          </div>
        ) : (
          <div className="space-y-1 text-[14px] leading-relaxed text-fg/85">
            {lyrics.lines.map((l, i) => (l.text ? <p key={i}>{l.text}</p> : <div key={i} className="h-3" />))}
          </div>
        )}
      </div>
    </section>
  );
}

function Empty({ text }: { text: string }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 px-2 text-center text-[12.5px] leading-relaxed text-muted">
      <Mic2 size={22} strokeWidth={1.5} className="text-faint" />
      {text}
    </div>
  );
}
