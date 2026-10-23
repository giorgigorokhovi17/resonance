import { Loader2, Mic2, Search, X } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { api } from "../lib/api";
import { displayArtist } from "../lib/format";
import type { Track } from "../lib/types";
import { useLiveCover, useLiveMotion } from "../store/library";
import { removeFoundLyrics, searchLyrics, useLyrics } from "../store/lyrics";
import { livePosition, usePlayer } from "../store/player";
import { useUi } from "../store/ui";
import { Cover } from "./Cover";

/** The playing song, or the restored queue's current one while stopped. */
function usePlayingTrack(): Track | null {
  return usePlayer((s) => s.status?.track ?? s.queue.items.find((i) => i.uid === s.queue.currentUid)?.track ?? null);
}

/**
 * The playing song's lyrics: from a `.lrc` file next to it, its tags, or found online when the
 * user asked. Timed lyrics follow the song (click a line to jump there); plain lyrics are shown as
 * written. `big` is the lyrics window's larger text.
 */
export function LyricsBody({ track, big = false }: { track: Track | null; big?: boolean }) {
  const path = track?.path ?? null;
  const { lyrics, searching, notFound } = useLyrics(path);
  const [current, setCurrent] = useState(-1);
  const scroller = useRef<HTMLDivElement>(null);
  const userScrolled = useRef(0);

  useEffect(() => {
    scroller.current?.scrollTo({ top: 0 });
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
    <div
      ref={scroller}
      onWheel={() => (userScrolled.current = Date.now())}
      className={`relative min-h-0 flex-1 overflow-y-auto ${big ? "px-8 py-4" : "px-4 py-3"}`}
    >
      {!track ? (
        <Empty text="Play a song to see its lyrics here." />
      ) : lyrics === undefined ? null : !lyrics ? (
        <Empty text={notFound ?? "No lyrics for this song yet."}>
          <button
            onClick={() => path && searchLyrics(path)}
            disabled={searching}
            className="mt-1 inline-flex items-center gap-1.5 rounded-full px-3.5 py-1.5 text-[12.5px] font-semibold accent-gradient text-on-accent transition-opacity disabled:opacity-60"
          >
            {searching ? <Loader2 size={14} className="animate-spin" /> : <Search size={14} />}
            {searching ? "Searching…" : notFound ? "Search again" : "Search lyrics"}
          </button>
          <span className="max-w-[300px] text-[11.5px] text-faint">
            Searches LRCLIB, a free lyrics site, with the song's title, artist and length. The song file isn't changed.
          </span>
        </Empty>
      ) : (
        <>
          {lyrics.synced ? (
            <div className="space-y-1.5 py-[30%]">
              {lyrics.lines.map((l, i) => (
                <button
                  key={i}
                  data-line={i}
                  onClick={() => api.seek((l.timeMs ?? 0) / 1000)}
                  title="Play from here"
                  className={`block w-full rounded px-1 text-left font-semibold leading-snug transition-colors duration-300 hover:text-fg ${
                    big ? "text-[22px]" : "text-[15px]"
                  } ${i === current ? "text-fg" : i < current ? "text-fg/35" : "text-fg/55"}`}
                >
                  {l.text || "♪"}
                </button>
              ))}
            </div>
          ) : (
            <div className={`space-y-1 leading-relaxed text-fg/85 ${big ? "text-[17px]" : "text-[14px]"}`}>
              {lyrics.lines.map((l, i) => (l.text ? <p key={i}>{l.text}</p> : <div key={i} className="h-3" />))}
            </div>
          )}
          {lyrics.source === "lrclib" && path && (
            <div className="flex items-center justify-center gap-2 pb-2 pt-4 text-[11.5px] text-faint">
              Lyrics from LRCLIB
              <span aria-hidden>·</span>
              <button className="underline hover:text-fg" onClick={() => removeFoundLyrics(path)} title="Forget these lyrics">
                Wrong lyrics? Remove
              </button>
            </div>
          )}
        </>
      )}
    </div>
  );
}

/** The lyrics column of the "Cover and lyrics" playlist page. */
export function LyricsPanel({ className = "" }: { className?: string }) {
  const track = usePlayingTrack();
  const coverId = useLiveCover(track);
  const motionId = useLiveMotion(track);
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
      <LyricsBody track={track} />
    </section>
  );
}

/** The lyrics window, opened from the player bar (or L). */
export function LyricsDialog() {
  const open = useUi((s) => s.lyricsOpen);
  const setOpen = useUi((s) => s.setLyricsOpen);
  const track = usePlayingTrack();
  const coverId = useLiveCover(track);
  const motionId = useLiveMotion(track);
  if (!open) return null;
  return (
    <div className="fixed inset-0 z-[110] flex items-center justify-center bg-black/50 p-6 backdrop-blur-sm animate-in" onMouseDown={() => setOpen(false)}>
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Lyrics"
        className="flex h-[min(720px,82vh)] w-[min(620px,92vw)] flex-col overflow-hidden rounded-2xl bg-panel shadow-2xl ring-1 ring-line"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="flex shrink-0 items-center gap-4 border-b border-line/70 p-4">
          <Cover coverId={coverId} motionId={motionId} motion="tile" className="h-14 w-14 shrink-0 shadow-md" iconSize={22} />
          <div className="min-w-0 flex-1">
            <div className="text-[10.5px] font-semibold uppercase tracking-wider text-muted">Lyrics</div>
            <div className="truncate text-[16px] font-semibold">{track?.title ?? "Nothing playing"}</div>
            {track && <div className="truncate text-[13px] text-muted">{displayArtist(track)}</div>}
          </div>
          <button
            aria-label="Close lyrics"
            title="Close (Esc)"
            onClick={() => setOpen(false)}
            className="rounded-full p-2 text-muted transition-colors hover:bg-hover hover:text-fg"
          >
            <X size={18} />
          </button>
        </div>
        <LyricsBody track={track} big />
      </div>
    </div>
  );
}

function Empty({ text, children }: { text: string; children?: ReactNode }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 px-2 text-center text-[12.5px] leading-relaxed text-muted">
      <Mic2 size={22} strokeWidth={1.5} className="text-faint" />
      {text}
      {children}
    </div>
  );
}
