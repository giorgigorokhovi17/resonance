import { BarChart3, Clock, Flame, Play, Trash2 } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { api } from "../lib/api";
import { playTracks } from "../lib/actions";
import type { Stats, Track } from "../lib/types";
import { useLiveCover } from "../store/library";
import { usePlayer } from "../store/player";
import { Cover } from "../components/Cover";
import { confirmDialog } from "../components/Dialog";
import { Segmented } from "./SettingsView";

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

function minutes(secs: number) {
  return Math.round(secs / 60).toLocaleString();
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
    } catch (e) {
      usePlayer.getState().pushToast("error", String(e));
    }
  };

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
            <div className="accent-gradient grid grid-cols-2 gap-px overflow-hidden rounded-2xl text-on-accent shadow-lg md:grid-cols-4">
              <Big value={minutes(stats.seconds)} label="minutes listened" />
              <Big value={stats.plays.toLocaleString()} label="plays" />
              <Big value={stats.songs.toLocaleString()} label="different songs" />
              <Big value={stats.artists.toLocaleString()} label="artists" />
            </div>

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
                          className="accent-gradient h-full rounded-full"
                          style={{ width: `${(a.seconds / (stats.topArtists[0]?.seconds || 1)) * 100}%` }}
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

            <div className="grid gap-6 lg:grid-cols-2">
              <Card title="When you listen">
                <Bars values={stats.byHour} labels={stats.byHour.map((_, h) => (h % 6 === 0 ? `${h}:00` : ""))} tip={(h) => `${h}:00–${h + 1}:00`} />
              </Card>
              <Card title="By day of the week">
                <Bars values={stats.byWeekday} labels={WEEKDAYS} tip={(d) => WEEKDAYS[d]} />
              </Card>
            </div>

            <div className="grid gap-4 sm:grid-cols-3">
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

function Big({ value, label }: { value: string; label: string }) {
  return (
    <div className="bg-black/10 px-6 py-5">
      <div className="font-display text-[32px] font-bold leading-none tabular">{value}</div>
      <div className="mt-1.5 text-[12.5px] font-medium opacity-80">{label}</div>
    </div>
  );
}

function Card({ title, action, children }: { title: string; action?: ReactNode; children: ReactNode }) {
  return (
    <section className="rounded-xl bg-panel p-5 ring-1 ring-line">
      <div className="mb-3 flex items-center justify-between gap-3">
        <h2 className="text-[15px] font-semibold">{title}</h2>
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
            className="flex-1 rounded-t bg-accent/80 transition-opacity hover:opacity-100"
            style={{ height: `${Math.max(2, (v / max) * 100)}%`, opacity: v > 0 ? 0.85 : 0.2 }}
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
