import {
  Calendar,
  ChevronLeft,
  ChevronRight,
  Download,
  Flame,
  Loader2,
  Pause,
  Play,
  RotateCcw,
  Sparkles,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useCoverColor } from "../hooks/useCoverColor";
import { playTracks } from "../lib/actions";
import { saveRecapImage } from "../lib/recapImage";
import type { Stats, Track } from "../lib/types";
import { useLiveCover } from "../store/library";
import { usePlayer } from "../store/player";
import { Ambient } from "./Ambient";
import { Cover, coverUrl } from "./Cover";
import { GenreMix, HourClock, hm, useCountUp } from "./StatsCharts";

const SLIDE_MS = 6000;

type SlideId = "intro" | "artist" | "songs" | "album" | "hours" | "topDay" | "genres" | "summary";

function hours(secs: number) {
  const h = secs / 3600;
  return h >= 10 ? `${Math.round(h)} h` : h >= 1 ? `${h.toFixed(1)} h` : `${Math.round(secs / 60)} min`;
}

function listenerLabel(byHour: number[]) {
  const h = byHour.indexOf(Math.max(...byHour));
  if (h >= 5 && h < 12) return "A morning listener";
  if (h >= 12 && h < 17) return "An afternoon listener";
  if (h >= 17 && h < 21) return "An evening listener";
  return "A night owl";
}

function buildSlides(stats: Stats): SlideId[] {
  const list: SlideId[] = ["intro"];
  if (stats.topArtists.length > 0) list.push("artist");
  if (stats.topSongs.length > 0) list.push("songs");
  if (stats.topAlbums.length > 0) list.push("album");
  if (stats.byHour.some((v) => v > 0)) list.push("hours");
  if (stats.topDay) list.push("topDay");
  if (stats.genres.length > 0) list.push("genres");
  list.push("summary");
  return list;
}

function SongArt({ track, className, rounded = "rounded-lg", size = "thumb", iconSize = 20 }: {
  track: Track | null;
  className: string;
  rounded?: string;
  size?: "thumb" | "full";
  iconSize?: number;
}) {
  const coverId = useLiveCover(track);
  return <Cover coverId={coverId} size={size} className={`shrink-0 ${className}`} rounded={rounded} iconSize={iconSize} />;
}

/**
 * Full-screen, story-style look back at a listening period.
 * Click left/right or use arrow keys to step, Space to pause, Esc to close.
 */
export function RecapOverlay({
  stats,
  title,
  periodSlug,
  onClose,
}: {
  stats: Stats;
  title: string;
  periodSlug: string;
  onClose: () => void;
}) {
  const slides = useMemo(() => buildSlides(stats), [stats]);
  const [index, setIndex] = useState(0);
  const [started, setStarted] = useState(false);
  const [paused, setPaused] = useState(false);
  const [progress, setProgress] = useState(0);
  const [saving, setSaving] = useState(false);
  const elapsedRef = useRef(0);

  const heroTrack = stats.topAlbums[0]?.track ?? stats.topSongs[0]?.track ?? null;
  const heroCoverId = useLiveCover(heroTrack);
  const coverColor = useCoverColor(heroCoverId);
  const bgUrl = coverUrl(heroCoverId, "full");
  const ambientIntensity = usePlayer((s) => s.settings?.appearance.ambientIntensity ?? 1);

  const playable = useMemo(
    () => stats.topSongs.map((s) => s.track).filter((t): t is Track => !!t && t.id > 0),
    [stats.topSongs],
  );

  const goTo = useCallback(
    (nextIdx: number) => {
      const clamped = Math.max(0, Math.min(slides.length - 1, nextIdx));
      elapsedRef.current = 0;
      setProgress(0);
      setIndex(clamped);
    },
    [slides.length],
  );

  const next = useCallback(() => {
    if (!started) setStarted(true);
    if (index < slides.length - 1) goTo(index + 1);
  }, [goTo, index, slides.length, started]);

  const prev = useCallback(() => {
    if (index > 0) goTo(index - 1);
  }, [goTo, index]);

  const handleStart = useCallback(() => {
    setStarted(true);
    setPaused(false);
    // Start the period's #1 song only if nothing is currently playing.
    if (usePlayer.getState().status?.state !== "playing" && playable.length > 0) {
      void playTracks(playable, 0);
    }
    if (slides.length > 1) goTo(1);
  }, [goTo, playable, slides.length]);

  const handleReplay = useCallback(() => {
    setStarted(true);
    setPaused(false);
    goTo(0);
  }, [goTo]);

  // Auto-advance timer: 6 seconds per slide while running.
  useEffect(() => {
    if (!started || paused) return;
    let raf = 0;
    let last = performance.now();
    const tick = (now: number) => {
      const dt = Math.min(200, now - last);
      last = now;
      elapsedRef.current += dt;
      const pct = Math.min(1, elapsedRef.current / SLIDE_MS);
      setProgress(pct);
      if (pct >= 1) {
        if (index < slides.length - 1) {
          elapsedRef.current = 0;
          setProgress(0);
          setIndex((i) => Math.min(slides.length - 1, i + 1));
        }
        return;
      }
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [started, paused, index, slides.length]);

  // Capture-phase shortcuts so Space, arrows and Esc control the recap instead of the player behind it.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopImmediatePropagation();
        onClose();
      } else if (e.key === "ArrowRight") {
        e.preventDefault();
        e.stopImmediatePropagation();
        next();
      } else if (e.key === "ArrowLeft") {
        e.preventDefault();
        e.stopImmediatePropagation();
        prev();
      } else if (e.key === " ") {
        e.preventDefault();
        e.stopImmediatePropagation();
        if (started) setPaused((p) => !p);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [next, onClose, prev, started]);

  const handleSurfaceClick = (e: React.MouseEvent<HTMLDivElement>) => {
    const target = e.target as HTMLElement | null;
    if (target?.closest("button, a, [data-recap-interactive]")) return;
    const rect = e.currentTarget.getBoundingClientRect();
    if (e.clientX >= rect.left + rect.width / 2) next();
    else prev();
  };

  const handleSave = async () => {
    if (saving) return;
    setSaving(true);
    try {
      const outcome = await saveRecapImage(
        {
          title,
          stats,
          coverId: heroCoverId ?? null,
          coverColor,
          listenerLabel: listenerLabel(stats.byHour),
        },
        `undertone-recap-${periodSlug}.png`,
      );
      if (outcome === "saved") usePlayer.getState().pushToast("info", "Recap image saved.");
      else if (outcome === "downloaded") usePlayer.getState().pushToast("info", "Recap image downloaded.");
    } catch (e) {
      usePlayer.getState().pushToast("error", String(e));
    } finally {
      setSaving(false);
    }
  };

  const slide = slides[index];
  const label = listenerLabel(stats.byHour);

  return createPortal(
    <div
      role="dialog"
      aria-modal="true"
      aria-label={`Recap: ${title}`}
      onClick={handleSurfaceClick}
      className="fixed inset-0 z-[120] flex flex-col overflow-hidden bg-bg text-fg select-none animate-in motion-reduce:transition-none"
      style={{
        backgroundColor: coverColor
          ? `color-mix(in srgb, ${coverColor} ${Math.round(38 * ambientIntensity)}%, var(--c-bg))`
          : undefined,
      }}
    >
      {/* Ambient / blurred-artwork backdrop */}
      {bgUrl && (
        <div className="pointer-events-none absolute inset-0" style={{ opacity: ambientIntensity }}>
          <div
            className="absolute -inset-24 scale-110 bg-cover bg-center opacity-55 dark:opacity-45"
            style={{ backgroundImage: `url("${bgUrl}")`, filter: "blur(var(--np-blur, 90px)) saturate(1.4)" }}
          />
        </div>
      )}
      <Ambient color={coverColor} />
      <div className="pointer-events-none absolute inset-0 bg-gradient-to-b from-bg/40 via-bg/60 to-bg/92" />

      {/* Top segmented story progress bar + controls */}
      <header className="relative z-10 mx-auto w-full max-w-3xl px-6 pt-5">
        <div className="flex gap-1.5" role="tablist" aria-label="Recap slides">
          {slides.map((id, i) => {
            const fill = i < index ? 1 : i === index ? (started ? progress : 0) : 0;
            return (
              <button
                key={id}
                type="button"
                aria-label={`Slide ${i + 1} of ${slides.length}`}
                aria-current={i === index ? "step" : undefined}
                onClick={(e) => {
                  e.stopPropagation();
                  if (!started) setStarted(true);
                  goTo(i);
                }}
                className="group py-2 flex-1 cursor-pointer"
              >
                <span className="block h-1 w-full overflow-hidden rounded-full bg-fg/20">
                  <span
                    className="block h-full rounded-full bg-fg"
                    style={{ width: `${fill * 100}%` }}
                  />
                </span>
              </button>
            );
          })}
        </div>

        <div className="mt-1 flex items-center justify-between gap-4">
          <div className="flex items-center gap-2 text-[12px] font-medium text-muted">
            <Sparkles size={14} className="text-accent" />
            <span>{title}</span>
            {started && paused && <span className="rounded-full bg-panel/80 px-2 py-0.5 text-[11px] text-fg ring-1 ring-line">Paused</span>}
          </div>
          <div className="flex items-center gap-1.5">
            {started && (
              <button
                type="button"
                onClick={() => setPaused((p) => !p)}
                title={paused ? "Resume (Space)" : "Pause (Space)"}
                aria-label={paused ? "Resume" : "Pause"}
                className="rounded-full bg-panel/75 p-2 text-fg/80 ring-1 ring-line backdrop-blur-md hover:bg-hover hover:text-fg"
              >
                {paused ? <Play size={15} fill="currentColor" strokeWidth={0} /> : <Pause size={15} />}
              </button>
            )}
            <button
              type="button"
              onClick={onClose}
              title="Close (Esc)"
              aria-label="Close recap"
              className="rounded-full bg-panel/75 p-2 text-fg/80 ring-1 ring-line backdrop-blur-md hover:bg-hover hover:text-fg"
            >
              <X size={16} />
            </button>
          </div>
        </div>
      </header>

      {/* Main slide canvas */}
      <main className="relative z-10 flex min-h-0 flex-1 items-center justify-center overflow-y-auto px-6 py-6">
        <div key={slide} className="w-full max-w-2xl">
          {slide === "intro" && (
            <IntroSlide
              title={title}
              stats={stats}
              started={started}
              topSongTitle={playable[0]?.title ?? null}
              onStart={handleStart}
            />
          )}
          {slide === "artist" && <ArtistSlide stats={stats} />}
          {slide === "songs" && <SongsSlide stats={stats} />}
          {slide === "album" && <AlbumSlide stats={stats} />}
          {slide === "hours" && <HoursSlide stats={stats} label={label} />}
          {slide === "topDay" && <TopDaySlide stats={stats} />}
          {slide === "genres" && <GenresSlide stats={stats} />}
          {slide === "summary" && (
            <SummarySlide
              title={title}
              stats={stats}
              heroTrack={heroTrack}
              label={label}
              saving={saving}
              onSave={handleSave}
              onReplay={handleReplay}
              onClose={onClose}
            />
          )}
        </div>
      </main>

      {/* Bottom hint & step buttons */}
      <footer className="relative z-10 mx-auto flex w-full max-w-3xl items-center justify-between px-6 pb-5 text-[12px] text-muted">
        <button
          type="button"
          onClick={prev}
          disabled={index === 0}
          aria-label="Previous slide"
          className="inline-flex items-center gap-1 rounded-full px-3 py-1.5 hover:bg-hover hover:text-fg disabled:pointer-events-none disabled:opacity-30"
        >
          <ChevronLeft size={15} /> Back
        </button>
        <span className="tabular text-faint">
          {index + 1} / {slides.length}
        </span>
        <button
          type="button"
          onClick={next}
          disabled={index === slides.length - 1}
          aria-label="Next slide"
          className="inline-flex items-center gap-1 rounded-full px-3 py-1.5 hover:bg-hover hover:text-fg disabled:pointer-events-none disabled:opacity-30"
        >
          Next <ChevronRight size={15} />
        </button>
      </footer>
    </div>,
    document.body,
  );
}

// ---- Individual slides -----------------------------------------------------------------------

function IntroSlide({
  title,
  stats,
  started,
  topSongTitle,
  onStart,
}: {
  title: string;
  stats: Stats;
  started: boolean;
  topSongTitle: string | null;
  onStart: () => void;
}) {
  const minutes = Math.round(stats.seconds / 60);
  const shown = useCountUp(minutes, 1100);
  return (
    <div className="flex flex-col items-center text-center">
      <div className="recap-rise text-[12px] font-semibold uppercase tracking-[0.18em] text-accent">
        {title}
      </div>
      <div
        className="recap-rise mt-4 font-display text-[clamp(64px,12vw,124px)] font-bold leading-none tabular accent-text"
        style={{ animationDelay: "80ms" }}
      >
        {Math.round(shown).toLocaleString()}
      </div>
      <div className="recap-rise mt-3 font-display text-[22px] font-semibold" style={{ animationDelay: "160ms" }}>
        minutes listened
      </div>
      <div className="recap-rise mt-1.5 text-[14px] text-muted tabular" style={{ animationDelay: "220ms" }}>
        {hours(stats.seconds)} across {stats.plays.toLocaleString()} {stats.plays === 1 ? "play" : "plays"}
      </div>
      {!started && (
        <div className="recap-rise mt-9 flex flex-col items-center gap-2.5" style={{ animationDelay: "300ms" }}>
          <button
            type="button"
            onClick={onStart}
            className="accent-gradient inline-flex items-center gap-2 rounded-full px-7 py-3 text-[15px] font-semibold text-on-accent shadow-lg transition-transform hover:scale-[1.02] active:scale-[0.98] motion-reduce:transition-none"
          >
            <Play size={16} fill="currentColor" strokeWidth={0} /> Start
          </button>
          {topSongTitle && (
            <span className="max-w-xs truncate text-[12px] text-faint">
              Plays “{topSongTitle}” if nothing is playing
            </span>
          )}
        </div>
      )}
    </div>
  );
}

function ArtistSlide({ stats }: { stats: Stats }) {
  const top = stats.topArtists[0];
  if (!top) return null;
  const artistTrack = stats.topSongs.find((s) => s.artist === top.name && s.track)?.track ?? null;
  const rest = stats.topArtists.slice(1, 4);
  return (
    <div className="flex flex-col items-center text-center">
      <div className="recap-rise text-[12px] font-semibold uppercase tracking-[0.18em] text-accent">
        Your #1 artist
      </div>
      {artistTrack && (
        <div className="recap-scale mt-5" style={{ animationDelay: "80ms" }}>
          <SongArt track={artistTrack} size="full" className="h-32 w-32 shadow-2xl ring-1 ring-line" rounded="rounded-full" iconSize={38} />
        </div>
      )}
      <h2
        className="recap-rise mt-5 font-display text-[clamp(32px,6vw,60px)] font-bold leading-[1.08]"
        style={{ animationDelay: "140ms" }}
      >
        {top.name}
      </h2>
      <div className="recap-rise mt-3 font-display text-[22px] font-semibold text-accent tabular" style={{ animationDelay: "220ms" }}>
        {hours(top.seconds)}
      </div>
      <div className="recap-rise mt-1 text-[13.5px] text-muted tabular" style={{ animationDelay: "280ms" }}>
        {top.plays.toLocaleString()} {top.plays === 1 ? "play" : "plays"} · {top.songs.toLocaleString()} {top.songs === 1 ? "song" : "songs"}
      </div>
      {rest.length > 0 && (
        <div className="recap-rise mt-8 flex flex-wrap justify-center gap-2" style={{ animationDelay: "360ms" }}>
          {rest.map((a, i) => (
            <span
              key={a.name}
              className="inline-flex items-center gap-2 rounded-full bg-panel/75 px-3.5 py-1.5 text-[12.5px] ring-1 ring-line backdrop-blur-md"
            >
              <span className="tabular font-semibold text-faint">#{i + 2}</span>
              <span className="max-w-[180px] truncate font-medium">{a.name}</span>
              <span className="tabular text-muted">{hours(a.seconds)}</span>
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

function SongsSlide({ stats }: { stats: Stats }) {
  const top5 = stats.topSongs.slice(0, 5);
  return (
    <div>
      <div className="recap-rise text-center text-[12px] font-semibold uppercase tracking-[0.18em] text-accent">
        {top5.length >= 5 ? "Your top 5 songs" : "Your top songs"}
      </div>
      <ol className="mt-6 space-y-2.5">
        {top5.map((s, i) => (
          <li
            key={s.path}
            className="recap-fly flex items-center gap-3.5 rounded-xl bg-panel/80 p-3 ring-1 ring-line backdrop-blur-md"
            style={{ animationDelay: `${100 + i * 120}ms` }}
          >
            <span className="tabular w-6 shrink-0 text-right font-display text-[18px] font-bold text-accent">
              {i + 1}
            </span>
            <SongArt track={s.track} className="h-12 w-12 shadow-md" rounded="rounded-lg" iconSize={18} />
            <div className="min-w-0 flex-1">
              <div className="truncate text-[15px] font-semibold">{s.title}</div>
              <div className="truncate text-[12.5px] text-muted">{s.artist}</div>
            </div>
            <span className="tabular shrink-0 text-[12.5px] font-medium text-muted">
              {s.plays.toLocaleString()} {s.plays === 1 ? "play" : "plays"}
            </span>
          </li>
        ))}
      </ol>
    </div>
  );
}

function AlbumSlide({ stats }: { stats: Stats }) {
  const top = stats.topAlbums[0];
  if (!top) return null;
  return (
    <div className="flex flex-col items-center text-center">
      <div className="recap-rise text-[12px] font-semibold uppercase tracking-[0.18em] text-accent">
        Album on repeat
      </div>
      <div className="recap-scale mt-6" style={{ animationDelay: "80ms" }}>
        <SongArt
          track={top.track}
          size="full"
          className="aspect-square w-[min(260px,42vh)] shadow-2xl ring-1 ring-line"
          rounded="rounded-2xl"
          iconSize={64}
        />
      </div>
      <h2
        className="recap-rise mt-6 max-w-xl font-display text-[clamp(24px,4vw,40px)] font-bold leading-tight"
        style={{ animationDelay: "200ms" }}
      >
        {top.album}
      </h2>
      <div className="recap-rise mt-1 text-[15px] text-muted" style={{ animationDelay: "260ms" }}>
        {top.artist}
      </div>
      <div className="recap-rise mt-3 text-[13.5px] font-medium text-accent tabular" style={{ animationDelay: "320ms" }}>
        {hours(top.seconds)} · {top.plays.toLocaleString()} {top.plays === 1 ? "play" : "plays"}
      </div>
    </div>
  );
}

function HoursSlide({ stats, label }: { stats: Stats; label: string }) {
  return (
    <div className="flex flex-col items-center text-center">
      <div className="recap-rise text-[12px] font-semibold uppercase tracking-[0.18em] text-accent">
        When you listen
      </div>
      <h2
        className="recap-rise mt-2 font-display text-[clamp(28px,5vw,48px)] font-bold leading-tight"
        style={{ animationDelay: "80ms" }}
      >
        {label}
      </h2>
      <div
        className="recap-rise mt-6 rounded-2xl bg-panel/75 p-6 ring-1 ring-line backdrop-blur-md"
        style={{ animationDelay: "160ms" }}
      >
        <HourClock byHour={stats.byHour} />
      </div>
    </div>
  );
}

function TopDaySlide({ stats }: { stats: Stats }) {
  if (!stats.topDay) return null;
  const [daySec, listenedSec] = stats.topDay;
  const dateLabel = new Date(daySec * 1000).toLocaleDateString(undefined, {
    weekday: "long",
    month: "long",
    day: "numeric",
    year: "numeric",
  });
  return (
    <div className="flex flex-col items-center text-center">
      <div className="recap-rise inline-flex items-center gap-1.5 text-[12px] font-semibold uppercase tracking-[0.18em] text-accent">
        <Calendar size={14} /> Your biggest day
      </div>
      <div
        className="recap-rise mt-5 font-display text-[clamp(52px,10vw,104px)] font-bold leading-none tabular accent-text"
        style={{ animationDelay: "80ms" }}
      >
        {hm(listenedSec)}
      </div>
      <h2
        className="recap-rise mt-4 font-display text-[clamp(20px,3vw,30px)] font-semibold"
        style={{ animationDelay: "160ms" }}
      >
        {dateLabel}
      </h2>
      {stats.longestStreak > 1 && (
        <div
          className="recap-rise mt-7 inline-flex items-center gap-2 rounded-full bg-panel/75 px-4 py-2 text-[13px] ring-1 ring-line backdrop-blur-md"
          style={{ animationDelay: "240ms" }}
        >
          <Flame size={15} className="text-accent" />
          <span className="text-muted">Longest streak:</span>
          <span className="font-semibold tabular">{stats.longestStreak} days</span>
        </div>
      )}
    </div>
  );
}

function GenresSlide({ stats }: { stats: Stats }) {
  const total = stats.genres.reduce((n, [, s]) => n + s, 0) || 1;
  const top = stats.genres[0];
  return (
    <div className="flex flex-col items-center">
      <div className="recap-rise text-[12px] font-semibold uppercase tracking-[0.18em] text-accent">
        Your genre mix
      </div>
      {top && (
        <h2
          className="recap-rise mt-2 text-center font-display text-[clamp(28px,5vw,48px)] font-bold leading-tight"
          style={{ animationDelay: "80ms" }}
        >
          {top[0]} <span className="text-muted tabular">· {Math.round((top[1] / total) * 100)}%</span>
        </h2>
      )}
      <div
        className="recap-rise mt-6 w-full max-w-lg rounded-2xl bg-panel/80 p-6 ring-1 ring-line backdrop-blur-md"
        style={{ animationDelay: "160ms" }}
      >
        <GenreMix genres={stats.genres} />
      </div>
    </div>
  );
}

function SummarySlide({
  title,
  stats,
  heroTrack,
  label,
  saving,
  onSave,
  onReplay,
  onClose,
}: {
  title: string;
  stats: Stats;
  heroTrack: Track | null;
  label: string;
  saving: boolean;
  onSave: () => void;
  onReplay: () => void;
  onClose: () => void;
}) {
  const minutes = Math.round(stats.seconds / 60);
  const topArtist = stats.topArtists[0];
  const topSong = stats.topSongs[0];
  const topAlbum = stats.topAlbums[0];

  const five = [
    { label: "Minutes", value: minutes.toLocaleString() },
    { label: "Plays", value: stats.plays.toLocaleString() },
    { label: "Songs", value: stats.songs.toLocaleString() },
    { label: "Artists", value: stats.artists.toLocaleString() },
    { label: "Longest streak", value: `${stats.longestStreak}d` },
  ];

  return (
    <div className="flex flex-col items-center">
      <div className="recap-rise w-full max-w-lg rounded-2xl bg-panel/85 p-6 shadow-2xl ring-1 ring-line backdrop-blur-md">
        <div className="flex items-center gap-4">
          <SongArt track={heroTrack} size="full" className="h-20 w-20 shadow-lg ring-1 ring-line" rounded="rounded-xl" iconSize={28} />
          <div className="min-w-0 flex-1">
            <div className="text-[11px] font-semibold uppercase tracking-[0.16em] text-accent">Undertone recap</div>
            <h2 className="mt-0.5 truncate font-display text-[22px] font-bold leading-tight">{title}</h2>
            <div className="mt-0.5 text-[12.5px] text-muted">{label}</div>
          </div>
        </div>

        {/* Five headline numbers */}
        <div className="mt-5 grid grid-cols-5 gap-2">
          {five.map((item) => (
            <div key={item.label} className="rounded-xl bg-bg/70 px-2 py-2.5 text-center ring-1 ring-line">
              <div className="font-display text-[18px] font-bold leading-tight tabular">{item.value}</div>
              <div className="mt-0.5 truncate text-[10.5px] text-muted">{item.label}</div>
            </div>
          ))}
        </div>

        {/* Top picks */}
        <div className="mt-4 space-y-2 rounded-xl bg-bg/55 p-3.5 ring-1 ring-line">
          {topArtist && (
            <div className="flex items-baseline justify-between gap-3 text-[13px]">
              <span className="shrink-0 text-muted">Top artist</span>
              <span className="truncate font-semibold">{topArtist.name}</span>
            </div>
          )}
          {topSong && (
            <div className="flex items-baseline justify-between gap-3 text-[13px]">
              <span className="shrink-0 text-muted">Top song</span>
              <span className="truncate font-semibold">{topSong.title}</span>
            </div>
          )}
          {topAlbum && (
            <div className="flex items-baseline justify-between gap-3 text-[13px]">
              <span className="shrink-0 text-muted">Top album</span>
              <span className="truncate font-semibold">{topAlbum.album}</span>
            </div>
          )}
        </div>
      </div>

      <div className="recap-rise mt-6 flex flex-wrap items-center justify-center gap-2.5" style={{ animationDelay: "120ms" }}>
        <button
          type="button"
          onClick={onSave}
          disabled={saving}
          className="accent-gradient inline-flex items-center gap-2 rounded-full px-5 py-2.5 text-[13.5px] font-semibold text-on-accent shadow-md transition-opacity disabled:opacity-60 motion-reduce:transition-none"
        >
          {saving ? <Loader2 size={15} className="animate-spin motion-reduce:animate-none" /> : <Download size={15} />}
          {saving ? "Saving…" : "Save as image"}
        </button>
        <button
          type="button"
          onClick={onReplay}
          className="inline-flex items-center gap-1.5 rounded-full bg-panel/80 px-4 py-2.5 text-[13.5px] font-medium ring-1 ring-line backdrop-blur-md hover:bg-hover"
        >
          <RotateCcw size={14} /> Replay
        </button>
        <button
          type="button"
          onClick={onClose}
          className="inline-flex items-center gap-1.5 rounded-full bg-panel/80 px-4 py-2.5 text-[13.5px] font-medium ring-1 ring-line backdrop-blur-md hover:bg-hover"
        >
          Done
        </button>
      </div>
    </div>
  );
}
