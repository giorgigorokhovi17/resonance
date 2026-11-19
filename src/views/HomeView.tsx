import { Check, Disc3, Flame, Headphones, Music2, Pause, Play, PlusCircle, Shuffle } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { api } from "../lib/api";
import { playShuffled } from "../lib/actions";
import type { Stats, Track } from "../lib/types";
import { useCoverColor } from "../hooks/useCoverColor";
import { useLibrary, useLiveCover } from "../store/library";
import { usePlayer } from "../store/player";
import { usePlaylists } from "../store/playlists";
import { useUi } from "../store/ui";
import { Cover } from "../components/Cover";
import { GenreMix, TONES, Tile, hm, tint } from "../components/StatsCharts";
import { Segmented } from "./SettingsView";

const DAILY_GOAL_MIN = 30;
const DAILY_GOAL_SECS = DAILY_GOAL_MIN * 60;
const WEEKLY_GOAL_SECS = 3.5 * 3600;
const REFRESH_THROTTLE_MS = 2000;

type ListenRange = "7d" | "8w";

interface HomeStats {
  today: Stats;
  month: Stats;
  prevMonth: Stats;
  weeks8: Stats;
  days30: Stats;
}

function greeting(hour: number) {
  if (hour >= 5 && hour < 12) return "Good morning";
  if (hour >= 12 && hour < 17) return "Good afternoon";
  if (hour >= 17 && hour < 22) return "Good evening";
  return "Up late";
}

async function fetchHomeStats(): Promise<HomeStats> {
  const now = new Date();
  const to = Math.floor(now.getTime() / 1000) + 1;
  const todayStart = Math.floor(new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime() / 1000);
  const monthStart = Math.floor(new Date(now.getFullYear(), now.getMonth(), 1).getTime() / 1000);
  const prevMonthStart = Math.floor(new Date(now.getFullYear(), now.getMonth() - 1, 1).getTime() / 1000);
  const prevMonthTo = Math.min(prevMonthStart + (to - monthStart), monthStart);
  const weeks8Start = Math.floor(new Date(now.getFullYear(), now.getMonth(), now.getDate() - 55).getTime() / 1000);
  const days30Start = to - 30 * 86_400;

  const [today, month, prevMonth, weeks8, days30] = await Promise.all([
    api.getStats(todayStart, to),
    api.getStats(monthStart, to),
    api.getStats(prevMonthStart, prevMonthTo),
    api.getStats(weeks8Start, to),
    api.getStats(days30Start, to),
  ]);
  return { today, month, prevMonth, weeks8, days30 };
}

function monthDelta(current: number, previous: number, prevPlays: number): ReactNode {
  if (prevPlays <= 0 || previous <= 0) return undefined;
  const pct = Math.round(((current - previous) / previous) * 100);
  const sign = pct > 0 ? "+" : "";
  return <span className={pct >= 0 ? "text-ok" : "text-muted"}>{`${sign}${pct}% on last month`}</span>;
}

/** Personal listening room: what's playing, daily goal, monthly highlights and recent listening. */
export function HomeView() {
  const tracks = useLibrary((s) => s.tracks);
  const playlists = usePlaylists((s) => s.playlists);
  const status = usePlayer((s) => s.status);
  const currentUid = status?.currentUid ?? null;
  const setView = useUi((s) => s.setView);

  const [stats, setStats] = useState<HomeStats | null>(null);
  const [range, setRange] = useState<ListenRange>("7d");

  // Fetch on mount and whenever the playing track changes (throttled, never on a timer).
  const lastFetchAt = useRef(0);
  const timerRef = useRef<number | null>(null);
  useEffect(() => {
    let alive = true;
    const run = () => {
      lastFetchAt.current = Date.now();
      fetchHomeStats()
        .then((res) => {
          if (alive) setStats(res);
        })
        .catch(() => {
          /* ignore transient stats errors */
        });
    };
    const elapsed = Date.now() - lastFetchAt.current;
    if (lastFetchAt.current === 0 || elapsed >= REFRESH_THROTTLE_MS) {
      run();
    } else {
      timerRef.current = window.setTimeout(run, REFRESH_THROTTLE_MS - elapsed);
    }
    return () => {
      alive = false;
      if (timerRef.current != null) {
        clearTimeout(timerRef.current);
        timerRef.current = null;
      }
    };
  }, [currentUid]);

  const now = new Date();
  const dateText = now.toLocaleDateString(undefined, { weekday: "long", month: "long", day: "numeric" });

  const addedThisWeek = useMemo(() => {
    const cutoff = Math.floor(Date.now() / 1000) - 7 * 86_400;
    let count = 0;
    for (const t of tracks) {
      if (t.addedAt >= cutoff) count++;
    }
    return count;
  }, [tracks]);

  const openStats = () => setView({ kind: "stats" });
  const hasHistory = stats != null && stats.month.firstPlay != null;
  const isEmpty = stats != null && stats.month.firstPlay == null;

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-6xl space-y-6 px-8 pb-10 pt-6">
        <header className="home-in" style={{ animationDelay: "0ms" }}>
          <h1 className="font-display text-[32px] font-semibold leading-tight tracking-tight">{greeting(now.getHours())}</h1>
          <div className="mt-1 text-[13px] text-muted tabular">
            {dateText} · {tracks.length.toLocaleString()} {tracks.length === 1 ? "song" : "songs"} · {playlists.length.toLocaleString()}{" "}
            {playlists.length === 1 ? "playlist" : "playlists"}
          </div>
        </header>

        {isEmpty ? (
          <>
            <div className="home-in" style={{ animationDelay: "50ms" }}>
              <NowPlayingCard track={status?.track ?? null} playing={status?.state === "playing"} todaySecs={0} tracks={tracks} />
            </div>
            <section
              className="home-in flex flex-col items-center gap-3 rounded-2xl bg-panel px-6 py-14 text-center ring-1 ring-line"
              style={{ animationDelay: "100ms" }}
            >
              <Headphones size={28} strokeWidth={1.5} className="text-faint" />
              <div className="max-w-sm text-[14px] text-muted">Play some music and this page fills in with your listening.</div>
            </section>
          </>
        ) : (
          <>
            <div className="grid grid-cols-1 gap-5 min-[900px]:grid-cols-[minmax(0,2fr)_minmax(0,1fr)]">
              <div className="home-in" style={{ animationDelay: "50ms" }}>
                <NowPlayingCard
                  track={status?.track ?? null}
                  playing={status?.state === "playing"}
                  todaySecs={stats?.today.seconds ?? 0}
                  tracks={tracks}
                />
              </div>

              {hasHistory && (
                <div className="home-in grid grid-cols-2 gap-3.5" style={{ animationDelay: "100ms" }}>
                  <Tile
                    tone={TONES[0]}
                    icon={<Disc3 size={16} />}
                    label="Plays this month"
                    value={stats.month.plays}
                    sub={monthDelta(stats.month.plays, stats.prevMonth.plays, stats.prevMonth.plays)}
                    onClick={openStats}
                  />
                  <Tile
                    tone={TONES[1]}
                    icon={<Headphones size={16} />}
                    label="Hours this month"
                    value={stats.month.seconds / 3600}
                    format={(n) => `${n.toFixed(1)} h`}
                    sub={monthDelta(stats.month.seconds, stats.prevMonth.seconds, stats.prevMonth.plays)}
                    onClick={openStats}
                  />
                  <Tile
                    tone={TONES[2]}
                    icon={<PlusCircle size={16} />}
                    label="Added this week"
                    value={addedThisWeek}
                    sub={`${tracks.length.toLocaleString()} in library`}
                    onClick={openStats}
                  />
                  <Tile
                    tone={TONES[3]}
                    icon={<Flame size={16} />}
                    label="Current streak"
                    value={stats.month.currentStreak}
                    format={(n) => `${Math.round(n)} ${Math.round(n) === 1 ? "day" : "days"}`}
                    sub={`best ${stats.month.longestStreak}`}
                    onClick={openStats}
                  />
                </div>
              )}
            </div>

            {hasHistory && (
              <div className="grid grid-cols-1 gap-5 min-[900px]:grid-cols-[minmax(0,1.4fr)_minmax(0,1fr)]">
                <section className="home-in rounded-2xl bg-panel p-5 ring-1 ring-line" style={{ animationDelay: "150ms" }}>
                  <ListeningChart byDay={stats.weeks8.byDay} range={range} onRangeChange={setRange} />
                </section>

                <section className="home-in rounded-2xl bg-panel p-5 ring-1 ring-line" style={{ animationDelay: "200ms" }}>
                  <div className="mb-4 flex items-start justify-between gap-3">
                    <div className="min-w-0">
                      <h2 className="text-[15px] font-semibold">Your genre mix</h2>
                      <div className="truncate text-[12px] text-muted tabular">
                        {stats.days30.plays.toLocaleString()} {stats.days30.plays === 1 ? "play" : "plays"} in 30 days
                      </div>
                    </div>
                    <button
                      type="button"
                      onClick={openStats}
                      className="shrink-0 cursor-pointer text-[12.5px] font-medium text-accent hover:underline"
                    >
                      Stats →
                    </button>
                  </div>
                  {stats.days30.genres.length > 0 ? (
                    <GenreMix genres={stats.days30.genres} />
                  ) : (
                    <div className="py-8 text-center text-[12.5px] text-muted">None of your recent songs have a genre tag.</div>
                  )}
                </section>
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}

/** Hero card: current track, quick actions, daily listening ring and animated vinyl turntable. */
function NowPlayingCard({
  track,
  playing,
  todaySecs,
  tracks,
}: {
  track: Track | null;
  playing: boolean;
  todaySecs: number;
  tracks: Track[];
}) {
  const coverId = useLiveCover(track);
  const coverColor = useCoverColor(coverId);
  const heroTone = coverColor ?? "var(--c-accent)";

  const onPrimary = () => {
    if (track) {
      api.togglePlay().catch((e) => usePlayer.getState().pushToast("error", String(e)));
    } else {
      playShuffled(tracks);
    }
  };

  const subtitle = track ? [track.artist, track.album].filter(Boolean).join(" · ") : "Pick a song or shuffle your library";

  return (
    <section
      className="relative flex h-full flex-col justify-between gap-6 overflow-hidden rounded-2xl p-6 ring-1 ring-line sm:flex-row sm:items-center"
      style={{
        background: `linear-gradient(135deg, ${tint(heroTone, 24)} 0%, ${tint(heroTone, 10)} 100%)`,
      }}
    >
      <div className="flex min-w-0 flex-1 flex-col justify-between gap-5">
        <div className="min-w-0">
          <div className="inline-flex items-center gap-2 rounded-full bg-bg/65 px-3 py-1 text-[12px] font-medium ring-1 ring-line/70">
            {playing ? (
              <>
                <span className="flex h-3 items-end gap-[2px]" aria-hidden>
                  <span className="eq-bar h-full w-[2.5px] rounded-full bg-accent" style={{ animationDelay: "0ms" }} />
                  <span className="eq-bar h-full w-[2.5px] rounded-full bg-accent" style={{ animationDelay: "180ms" }} />
                  <span className="eq-bar h-full w-[2.5px] rounded-full bg-accent" style={{ animationDelay: "340ms" }} />
                </span>
                <span>Now spinning</span>
              </>
            ) : (
              <>
                <span className="h-2 w-2 rounded-full bg-faint" aria-hidden />
                <span className="text-muted">{track ? "Paused" : "Ready to play"}</span>
              </>
            )}
          </div>

          <h2
            className="mt-3 truncate font-display text-[32px] font-bold leading-[1.12] tracking-tight sm:text-[34px]"
            title={track?.title ?? undefined}
          >
            {track ? track.title : "Nothing playing"}
          </h2>
          <div className="mt-1 truncate text-[14px] text-muted" title={subtitle}>
            {subtitle}
          </div>

          <div className="mt-5 flex flex-wrap items-center gap-2.5">
            <button
              type="button"
              onClick={onPrimary}
              disabled={!track && tracks.length === 0}
              className="inline-flex cursor-pointer items-center gap-2 rounded-full bg-accent px-5 py-2.5 text-[13px] font-semibold text-on-accent shadow-sm transition-opacity hover:opacity-90 disabled:cursor-default disabled:opacity-40"
            >
              {track ? (
                playing ? (
                  <>
                    <Pause size={15} fill="currentColor" strokeWidth={0} /> Pause
                  </>
                ) : (
                  <>
                    <Play size={15} fill="currentColor" strokeWidth={0} /> Play
                  </>
                )
              ) : (
                <>
                  <Play size={15} fill="currentColor" strokeWidth={0} /> Play something
                </>
              )}
            </button>
            <button
              type="button"
              onClick={() => playShuffled(tracks)}
              disabled={tracks.length === 0}
              className="inline-flex cursor-pointer items-center gap-2 rounded-full bg-panel/80 px-4 py-2.5 text-[13px] font-medium text-fg ring-1 ring-line transition-colors hover:bg-hover disabled:cursor-default disabled:opacity-40"
            >
              <Shuffle size={14} /> Shuffle all
            </button>
          </div>
        </div>

        <DailyGoalRing seconds={todaySecs} />
      </div>

      <VinylRecord coverId={coverId} hasTrack={!!track} playing={playing} />
    </section>
  );
}

/** SVG progress ring against the 30-minute daily listening goal. */
function DailyGoalRing({ seconds }: { seconds: number }) {
  const minutes = Math.round(seconds / 60);
  const remaining = Math.max(0, DAILY_GOAL_MIN - minutes);
  const met = minutes >= DAILY_GOAL_MIN;
  const pct = Math.min(1, seconds / DAILY_GOAL_SECS);
  const r = 17;
  const circ = 2 * Math.PI * r;

  return (
    <div className="flex items-center gap-3 pt-1">
      <div className="relative flex h-11 w-11 shrink-0 items-center justify-center">
        <svg viewBox="0 0 44 44" className="h-11 w-11 -rotate-90">
          <circle cx={22} cy={22} r={r} fill="none" stroke="color-mix(in srgb, var(--c-fg) 14%, transparent)" strokeWidth={3.5} />
          <circle
            cx={22}
            cy={22}
            r={r}
            fill="none"
            stroke={met ? "var(--c-ok)" : "var(--c-accent)"}
            strokeWidth={3.5}
            strokeLinecap="round"
            strokeDasharray={circ}
            strokeDashoffset={circ * (1 - pct)}
            className="transition-[stroke-dashoffset] duration-500 ease-out motion-reduce:transition-none"
          />
        </svg>
        <span className="absolute inset-0 flex items-center justify-center text-[10.5px] font-semibold tabular">
          {met ? <Check size={14} className="text-ok" strokeWidth={2.5} /> : `${Math.round(pct * 100)}%`}
        </span>
      </div>
      <div className="min-w-0">
        <div className="text-[13px] font-semibold tabular">{hm(seconds)} today</div>
        <div className="truncate text-[12px] text-muted">
          {met ? "Daily goal met. Nice." : `${remaining} more ${remaining === 1 ? "minute" : "minutes"} to your goal`}
        </div>
      </div>
    </div>
  );
}

/** Spinning vinyl disc with groove rings, cover-art centre label and articulated tonearm. */
function VinylRecord({ coverId, hasTrack, playing }: { coverId?: string; hasTrack: boolean; playing: boolean }) {
  return (
    <div className="relative mx-auto h-[204px] w-[224px] shrink-0 select-none sm:mx-0">
      {/* Dark grooved vinyl disc */}
      <div
        className="vinyl-spin absolute left-0 top-[6px] flex h-[192px] w-[192px] items-center justify-center rounded-full shadow-xl ring-1 ring-black/45"
        style={{
          animationPlayState: playing ? "running" : "paused",
          background:
            "radial-gradient(circle at 28% 25%, rgba(255,255,255,0.11), transparent 48%), repeating-radial-gradient(circle at center, #14151a 0px, #14151a 2px, #232630 2.6px, #14151a 3.6px)",
        }}
      >
        {/* Subtle inner groove sheen */}
        <div className="pointer-events-none absolute inset-3 rounded-full ring-1 ring-white/[0.06]" />
        {/* Round centre label */}
        <div className="relative h-[76px] w-[76px] overflow-hidden rounded-full bg-panel ring-2 ring-black/60 shadow-inner">
          {hasTrack && coverId ? (
            <Cover coverId={coverId} size="thumb" rounded="rounded-full" className="h-full w-full" iconSize={24} />
          ) : (
            <div className="flex h-full w-full items-center justify-center bg-elev text-faint/70">
              <Music2 size={22} strokeWidth={1.5} />
            </div>
          )}
        </div>
        {/* Spindle hole */}
        <div className="pointer-events-none absolute h-3 w-3 rounded-full bg-panel ring-2 ring-black/65" />
      </div>

      {/* Tonearm overlay anchored at top-right pivot */}
      <svg viewBox="0 0 224 204" className="pointer-events-none absolute inset-0 h-full w-full overflow-visible" aria-hidden>
        <g
          className="transition-transform duration-[600ms] ease-out motion-reduce:transition-none"
          style={{
            transformOrigin: "200px 30px",
            transform: playing ? "rotate(24deg)" : "rotate(-4deg)",
          }}
        >
          {/* Counterweight */}
          <rect x={196} y={10} width={8} height={12} rx={3} fill="var(--c-muted)" />
          {/* Arm tube with slight bend toward the record */}
          <path d="M 200 30 L 200 116 L 185 138" fill="none" stroke="var(--c-fg)" strokeOpacity={0.78} strokeWidth={3} strokeLinecap="round" strokeLinejoin="round" />
          {/* Headshell / cartridge in accent */}
          <rect x={178} y={133} width={11} height={17} rx={3} transform="rotate(34 183 141)" fill="var(--c-accent)" />
        </g>
        {/* Pivot base */}
        <circle cx={200} cy={30} r={12} fill="var(--c-panel)" stroke="var(--c-line)" strokeWidth={2} />
        <circle cx={200} cy={30} r={4.5} fill="var(--c-muted)" />
      </svg>
    </div>
  );
}

interface ListenBucket {
  key: string;
  label: string;
  fullLabel: string;
  secs: number;
  current: boolean;
}

function buildListenBuckets(byDay: [number, number][], range: ListenRange): ListenBucket[] {
  const now = new Date();
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  if (range === "7d") {
    const out: ListenBucket[] = [];
    for (let i = 6; i >= 0; i--) {
      const d = new Date(today.getFullYear(), today.getMonth(), today.getDate() - i);
      const start = d.getTime() / 1000;
      out.push({
        key: `d-${start}`,
        label: d.toLocaleDateString(undefined, { weekday: "short" }),
        fullLabel: d.toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" }),
        secs: 0,
        current: i === 0,
      });
    }
    for (const [day, secs] of byDay) {
      for (let i = 0; i < out.length; i++) {
        const start = Number(out[i].key.slice(2));
        if (Math.abs(day - start) <= 3600) {
          out[i].secs += secs;
          break;
        }
      }
    }
    return out;
  }

  // 8 weeks ending today (each bucket covers 7 days).
  const starts: number[] = [];
  const out: ListenBucket[] = [];
  for (let i = 7; i >= 0; i--) {
    const d = new Date(today.getFullYear(), today.getMonth(), today.getDate() - i * 7 - 6);
    const start = d.getTime() / 1000;
    const endD = new Date(today.getFullYear(), today.getMonth(), today.getDate() - i * 7);
    starts.push(start);
    out.push({
      key: `w-${start}`,
      label: d.toLocaleDateString(undefined, { day: "numeric", month: "short" }),
      fullLabel: `${d.toLocaleDateString(undefined, { month: "short", day: "numeric" })}–${endD.toLocaleDateString(undefined, { month: "short", day: "numeric" })}`,
      secs: 0,
      current: i === 0,
    });
  }
  for (const [day, secs] of byDay) {
    let i = starts.length - 1;
    while (i > 0 && starts[i] > day + 3600) i--;
    if (day + 3600 >= starts[i]) out[i].secs += secs;
  }
  return out;
}

/** 7-day / 8-week bar chart with daily/weekly goal line and hover tooltip. */
function ListeningChart({
  byDay,
  range,
  onRangeChange,
}: {
  byDay: [number, number][];
  range: ListenRange;
  onRangeChange: (r: ListenRange) => void;
}) {
  const [hover, setHover] = useState<number | null>(null);
  const items = useMemo(() => buildListenBuckets(byDay, range), [byDay, range]);
  const total = items.reduce((n, b) => n + b.secs, 0);
  const goalSecs = range === "7d" ? DAILY_GOAL_SECS : WEEKLY_GOAL_SECS;
  const maxSecs = Math.max(...items.map((b) => b.secs), goalSecs * 1.15, 60);
  const goalBottomPct = Math.min(95, (goalSecs / maxSecs) * 100);

  const sub =
    range === "7d"
      ? `${hm(total)} in the last 7 days · avg ${hm(total / 7)} a day`
      : `${hm(total)} in the last 8 weeks · avg ${hm(total / 8)} a week`;

  return (
    <div>
      <div className="mb-4 flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <h2 className="text-[15px] font-semibold">Your listening</h2>
          <div className="truncate text-[12px] text-muted tabular">{sub}</div>
        </div>
        <Segmented<ListenRange>
          value={range}
          onChange={onRangeChange}
          options={[
            { value: "7d", label: "7 days" },
            { value: "8w", label: "8 weeks" },
          ]}
        />
      </div>

      <div className="relative pt-6">
        <div className="relative flex h-40 items-end gap-2.5">
          {/* Dashed goal line */}
          <div
            className="pointer-events-none absolute inset-x-0 border-t border-dashed border-fg/25"
            style={{ bottom: `${goalBottomPct}%` }}
          >
            <span className="absolute -top-4 right-0 text-[10px] font-medium text-faint tabular">
              {range === "7d" ? `${DAILY_GOAL_MIN}m goal` : "3.5h goal"}
            </span>
          </div>

          {items.map((item, i) => {
            const pct = item.secs > 0 ? Math.max(4, (item.secs / maxSecs) * 100) : 2;
            const color = item.current ? "var(--c-accent)" : TONES[(i % (TONES.length - 1)) + 1];
            const isHovered = hover === i;
            return (
              <div
                key={`${range}-${item.key}`}
                className="relative flex h-full flex-1 flex-col justify-end"
                onMouseEnter={() => setHover(i)}
                onMouseLeave={() => setHover((cur) => (cur === i ? null : cur))}
              >
                {isHovered && (
                  <div
                    className={`pointer-events-none absolute -top-2 z-10 -translate-y-full whitespace-nowrap rounded-lg bg-fg px-2 py-1 text-[11.5px] font-medium text-bg shadow-lg tabular ${
                      i === 0 ? "left-0" : i === items.length - 1 ? "right-0" : "left-1/2 -translate-x-1/2"
                    }`}
                  >
                    {item.fullLabel} · {hm(item.secs)}
                  </div>
                )}
                <div
                  title={`${item.fullLabel}: ${hm(item.secs)}`}
                  className="grow-y w-full rounded-t-lg transition-opacity duration-150"
                  style={{
                    height: `${pct}%`,
                    background: color,
                    opacity: item.secs > 0 ? (isHovered ? 1 : 0.86) : 0.18,
                    animationDelay: `${i * 45}ms`,
                  }}
                />
              </div>
            );
          })}
        </div>

        <div className="mt-2 flex gap-2.5">
          {items.map((item) => (
            <div
              key={item.key}
              className={`flex-1 truncate text-center text-[11px] tabular ${item.current ? "font-semibold text-fg" : "text-faint"}`}
            >
              {item.label}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
