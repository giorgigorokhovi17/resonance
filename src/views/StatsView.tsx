import { BarChart3, Clock, Disc3, Flame, Headphones, Mic2, Play, Timer, Trash2, Zap } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { api } from "../lib/api";
import { playTracks } from "../lib/actions";
import type { Stats, Track } from "../lib/types";
import { useLiveCover } from "../store/library";
import { usePlayer } from "../store/player";
import { localMidnight, refreshGoal, useGoal } from "../store/goal";
import { Cover } from "../components/Cover";
import { GoalRing } from "../components/GoalRing";
import { confirmDialog } from "../components/Dialog";
import { Segmented } from "./SettingsView";
import { buckets, GenreMix, HourClock, OverTime, TONES, Tile, hm } from "../components/StatsCharts";

type Period = "month" | "days30" | "year" | "all";
// Remembered while the app runs.
let lastPeriod: Period = "year";

const WEEKDAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/** Unix seconds bounds of a period, in local time. */
function range(p: Period): [number, number] {
  const now = new Date();
  const to = Math.floor(now.getTime() / 1000) + 1;
  switch (p) {
    case "month":
      return [Math.floor(new Date(now.getFullYear(), now.getMonth(), 1).getTime() / 1000), to];
    case "days30":
      return [to - 30 * 86_400, to];
    case "year":
      return [Math.floor(new Date(now.getFullYear(), 0, 1).getTime() / 1000), to];
    case "all":
      return [0, to];
  }
}

function heading(p: Period) {
  const now = new Date();
  switch (p) {
    case "month":
      return `Your ${now.toLocaleString(undefined, { month: "long" })} in music`;
    case "days30":
      return "Your last 30 days in music";
    case "year":
      return `Your ${now.getFullYear()} in music`;
    case "all":
      return "All your listening";
  }
}

function hours(secs: number) {
  const h = secs / 3600;
  return h >= 10 ? `${Math.round(h)} h` : h >= 1 ? `${h.toFixed(1)} h` : `${Math.round(secs / 60)} min`;
}

/** Listening stats from the local play history: top songs, artists and albums, and when you listen. */
export function StatsView() {
  const [period, setPeriod] = useState<Period>(lastPeriod);
  const [stats, setStats] = useState<Stats | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  const goalMin = usePlayer((s) => s.settings?.listeningGoalMin ?? 0);
  const todaySeconds = useGoal((s) => s.seconds);
  const goalJustMet = useGoal((s) => s.justMet);

  useEffect(() => {
    lastPeriod = period;
    let alive = true;
    const [from, to] = range(period);
    api
      .getStats(from, to)
      .then((s) => alive && (setStats(s), setError(null)))
      .catch((e) => alive && setError(String(e)));
    return () => {
      alive = false;
    };
  }, [period, reload]);

  const clear = async () => {
    if (!(await confirmDialog("Clear listening history?", "This removes every recorded play. Stats start again from zero.", "Clear"))) return;
    try {
      await api.clearPlayHistory();
      setReload((n) => n + 1);
      void refreshGoal();
    } catch (e) {
      usePlayer.getState().pushToast("error", String(e));
    }
  };

  const [from, to] = range(period);
  const start = Math.max(from, stats?.firstPlay ?? from);
  const daysIn = Math.max(1, Math.ceil((to - start) / 86_400));
  const over = stats ? buckets(stats.byDay, start, to) : null;

  const playable = (stats?.topSongs ?? []).map((s) => s.track).filter((t): t is Track => !!t && t.id > 0);

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-5xl space-y-6 px-8 pb-10 pt-6">
        <div className="flex flex-wrap items-end justify-between gap-4">
          <div>
            <h1 className="font-display text-[28px] font-semibold leading-tight">{heading(period)}</h1>
            <div className="mt-1 text-[12.5px] text-muted">
              A song counts once you've heard 30 seconds of it. Your history stays on this PC.
            </div>
          </div>
          <Segmented
            value={period}
            onChange={setPeriod}
            options={[
              { value: "month", label: "This month" },
              { value: "days30", label: "30 days" },
              { value: "year", label: "This year" },
              { value: "all", label: "All time" },
            ]}
          />
        </div>

        {error ? (
          <div className="rounded-xl bg-panel p-6 text-[13px] text-muted ring-1 ring-line">{error}</div>
        ) : !stats ? null : stats.plays === 0 ? (
          <div className="flex flex-col items-center gap-3 rounded-xl bg-panel px-6 py-16 text-center ring-1 ring-line">
            <BarChart3 size={30} strokeWidth={1.5} className="text-faint" />
            <div className="text-[15px] font-semibold">Nothing here yet</div>
            <div className="max-w-sm text-[12.5px] leading-relaxed text-muted">
              {stats.firstPlay == null
                ? "Play some music. Your top songs, artists and listening habits show up here."
                : "No plays in this period. Try a longer one."}
            </div>
          </div>
        ) : (
          <>
            <div className="grid grid-cols-2 gap-4 md:grid-cols-4">
              <Tile tone={TONES[0]} icon={<Headphones size={16} />} label="Minutes listened" value={Math.round(stats.seconds / 60)} sub={`${hours(stats.seconds)} in all`} />
              <Tile tone={TONES[1]} icon={<Disc3 size={16} />} label="Plays" value={stats.plays} sub={`${stats.songs.toLocaleString()} different songs`} />
              <Tile tone={TONES[2]} icon={<Mic2 size={16} />} label="Artists heard" value={stats.artists} sub={`${stats.albums.toLocaleString()} albums`} />
              <Tile tone={TONES[3]} icon={<Timer size={16} />} label="A day, on average" value={stats.seconds / daysIn} format={hm} sub={`over ${daysIn} ${daysIn === 1 ? "day" : "days"}`} />
            </div>

            {over && over.list.length > 1 && (
              <Card title="Over time" sub={`Minutes listened each ${over.unit}`}>
                <OverTime list={over.list} unit={over.unit} />
              </Card>
            )}

            <div className="grid gap-6 lg:grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)]">
              <Card
                title="Top songs"
                action={
                  playable.length > 0 && (
                    <button
                      onClick={() => playTracks(playable)}
                      className="inline-flex items-center gap-1.5 rounded-full px-3 py-1 text-[12px] font-semibold ring-1 ring-line hover:bg-hover"
                    >
                      <Play size={12} fill="currentColor" strokeWidth={0} /> Play all
                    </button>
                  )
                }
              >
                <ol className="space-y-1">
                  {stats.topSongs.map((s, i) => (
                    <li key={s.path}>
                      <button
                        disabled={!s.track || s.track.id <= 0}
                        onClick={() => playTracks(playable, Math.max(0, playable.findIndex((t) => t.path === s.path)))}
                        title={s.track ? "Play" : "This song isn't in your library any more"}
                        className="group flex w-full items-center gap-3 rounded-lg px-2 py-1.5 text-left hover:bg-hover disabled:cursor-default disabled:hover:bg-transparent"
                      >
                        <span className="tabular w-5 shrink-0 text-right text-[13px] font-semibold text-faint">{i + 1}</span>
                        <SongCover track={s.track} className="h-10 w-10" />
                        <span className="min-w-0 flex-1">
                          <span className="block truncate text-[13.5px] font-medium">{s.title}</span>
                          <span className="block truncate text-[12px] text-muted">{s.artist}</span>
                        </span>
                        <span className="tabular shrink-0 text-[12px] text-muted">
                          {s.plays} {s.plays === 1 ? "play" : "plays"}
                        </span>
                      </button>
                    </li>
                  ))}
                </ol>
              </Card>

              <Card title="Top artists">
                <ol className="space-y-2.5">
                  {stats.topArtists.map((a, i) => (
                    <li key={a.name} className="px-1">
                      <div className="flex items-baseline gap-3">
                        <span className="tabular w-5 shrink-0 text-right text-[13px] font-semibold text-faint">{i + 1}</span>
                        <span className="min-w-0 flex-1 truncate text-[13.5px] font-medium">{a.name}</span>
                        <span className="tabular shrink-0 text-[12px] text-muted">{hours(a.seconds)}</span>
                      </div>
                      <div className="ml-8 mt-1 h-1.5 overflow-hidden rounded-full bg-fg/10">
                        <div
                          className="accent-gradient grow-x h-full rounded-full"
                          style={{ width: `${(a.seconds / (stats.topArtists[0]?.seconds || 1)) * 100}%`, animationDelay: `${i * 60}ms` }}
                        />
                      </div>
                    </li>
                  ))}
                </ol>
              </Card>
            </div>

            {stats.topAlbums.length > 0 && (
              <Card title="Top albums">
                <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-5">
                  {stats.topAlbums.slice(0, 10).map((a) => (
                    <div key={`${a.album}\u0000${a.artist}`} className="min-w-0">
                      <SongCover track={a.track} className="aspect-square w-full shadow-md" iconSize={28} />
                      <div className="mt-2 truncate text-[13px] font-medium">{a.album}</div>
                      <div className="truncate text-[12px] text-muted">
                        {a.artist} · {hours(a.seconds)}
                      </div>
                    </div>
                  ))}
                </div>
              </Card>
            )}

            <div className="grid gap-6 lg:grid-cols-3">
              <Card title="When you listen" sub="Minutes by hour">
                <HourClock byHour={stats.byHour} />
              </Card>
              <Card title="By day of the week">
                <Bars values={stats.byWeekday} labels={WEEKDAYS} tip={(d) => WEEKDAYS[d]} />
              </Card>
              <Card title="Your genre mix" sub={stats.genres.length ? `${stats.genres.length} ${stats.genres.length === 1 ? "genre" : "genres"}` : undefined}>
                {stats.genres.length ? (
                  <GenreMix genres={stats.genres} />
                ) : (
                  <div className="py-6 text-center text-[12.5px] text-muted">None of these songs have a genre tag.</div>
                )}
              </Card>
            </div>

            <div className={`grid gap-4 sm:grid-cols-2 ${goalMin > 0 ? "lg:grid-cols-3" : "lg:grid-cols-4"}`}>
              {goalMin > 0 && (
                <div className="flex items-center gap-4 rounded-xl bg-panel p-4 ring-1 ring-line sm:col-span-2">
                  <GoalRing minutes={todaySeconds / 60} goalMin={goalMin} size={52} pulseKey={goalJustMet} />
                  <div className="min-w-0">
                    <div className="tabular truncate text-[14px] font-semibold">
                      {Math.floor(todaySeconds / 60)} of {goalMin} minutes today
                    </div>
                    <div className="truncate text-[12px] text-muted">
                      Goal met {goalDaysLast7(stats.byDay, goalMin)} of the last 7 days
                    </div>
                  </div>
                </div>
              )}
              <Fact icon={<Zap size={18} />} value={`${stats.currentStreak} ${stats.currentStreak === 1 ? "day" : "days"}`} label="current streak" />
              <Fact icon={<Flame size={18} />} value={`${stats.longestStreak} ${stats.longestStreak === 1 ? "day" : "days"}`} label="longest listening streak" />
              {stats.topDay && (
                <Fact
                  icon={<BarChart3 size={18} />}
                  value={new Date(stats.topDay[0] * 1000).toLocaleDateString(undefined, { month: "long", day: "numeric", year: "numeric" })}
                  label={`your biggest day: ${hours(stats.topDay[1])}`}
                />
              )}
              <Fact icon={<Clock size={18} />} value={peakHour(stats.byHour)} label="your favourite time to listen" />
            </div>

            <div className="flex justify-center pt-2">
              <button onClick={clear} className="inline-flex items-center gap-1.5 text-[12px] text-faint hover:text-fg">
                <Trash2 size={13} /> Clear listening history
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

function goalDaysLast7(byDay: [number, number][], goalMin: number): number {
  if (goalMin <= 0) return 0;
  const need = goalMin * 60;
  // Half a day of slack keeps DST shifts from dropping the 7th day.
  const minTs = localMidnight() - 6 * 86_400 - 43_200;
  let met = 0;
  for (const [dayTs, secs] of byDay) {
    if (dayTs >= minTs && secs >= need) met++;
  }
  return met;
}

function peakHour(byHour: number[]) {
  const h = byHour.indexOf(Math.max(...byHour));
  if (h < 5) return "Late night";
  if (h < 12) return "Morning";
  if (h < 17) return "Afternoon";
  if (h < 21) return "Evening";
  return "Night";
}

function SongCover({ track, className, iconSize = 16 }: { track: Track | null; className: string; iconSize?: number }) {
  const coverId = useLiveCover(track);
  return <Cover coverId={coverId} className={`shrink-0 ${className}`} rounded="rounded-md" iconSize={iconSize} />;
}

function Card({ title, sub, action, children }: { title: string; sub?: string; action?: ReactNode; children: ReactNode }) {
  return (
    <section className="rounded-xl bg-panel p-5 ring-1 ring-line">
      <div className="mb-3 flex items-center justify-between gap-3">
        <div className="min-w-0">
          <h2 className="text-[15px] font-semibold">{title}</h2>
          {sub && <div className="truncate text-[12px] text-muted">{sub}</div>}
        </div>
        {action}
      </div>
      {children}
    </section>
  );
}

function Fact({ icon, value, label }: { icon: ReactNode; value: string; label: string }) {
  return (
    <div className="flex items-center gap-3 rounded-xl bg-panel p-4 ring-1 ring-line">
      <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-accent/15 text-accent">{icon}</span>
      <div className="min-w-0">
        <div className="truncate text-[14px] font-semibold">{value}</div>
        <div className="truncate text-[12px] text-muted">{label}</div>
      </div>
    </div>
  );
}

function Bars({ values, labels, tip }: { values: number[]; labels: string[]; tip: (i: number) => string }) {
  const max = Math.max(...values, 1);
  return (
    <div>
      <div className="flex h-32 items-end gap-1">
        {values.map((v, i) => (
          <div
            key={i}
            title={`${tip(i)}: ${hours(v)}`}
            className="grow-y flex-1 rounded-t-md transition-opacity hover:opacity-100"
            style={{
              height: `${Math.max(2, (v / max) * 100)}%`,
              opacity: v > 0 ? 0.85 : 0.2,
              background: v === max ? "var(--c-accent)" : TONES[(i % (TONES.length - 1)) + 1],
              animationDelay: `${i * 45}ms`,
            }}
          />
        ))}
      </div>
      <div className="mt-1.5 flex gap-1">
        {labels.map((l, i) => (
          <div key={i} className="flex-1 overflow-visible whitespace-nowrap text-[10.5px] text-faint">
            {l}
          </div>
        ))}
      </div>
    </div>
  );
}
