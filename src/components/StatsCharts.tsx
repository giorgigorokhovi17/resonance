import { useEffect, useRef, useState, type ReactNode } from "react";

// Soft tile colours, mixed into the panel colour so they suit every theme, dark or light.
export const TONES = ["var(--c-accent)", "#7fb38f", "#e0b84a", "#6fa8dc", "var(--c-accent2)"];

export function tint(tone: string, amount = 20) {
  return `color-mix(in oklab, ${tone} ${amount}%, var(--c-panel))`;
}

const reduced = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;

/** Counts up from 0 (first time) or the last value to `target`, easing out. */
export function useCountUp(target: number, ms = 900) {
  const [value, setValue] = useState(reduced() ? target : 0);
  const from = useRef(0);
  useEffect(() => {
    if (reduced()) {
      setValue(target);
      return;
    }
    const start = performance.now();
    const a = from.current;
    let raf = 0;
    const step = (now: number) => {
      const t = Math.min(1, (now - start) / ms);
      const v = a + (target - a) * (1 - Math.pow(1 - t, 3));
      setValue(v);
      from.current = v;
      if (t < 1) raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  }, [target, ms]);
  return value;
}

/** "2h 05m", "45m". */
export function hm(secs: number) {
  const m = Math.round(secs / 60);
  if (m < 60) return `${m}m`;
  return `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, "0")}m`;
}

/** A coloured stat tile whose number counts up. */
export function Tile({
  icon,
  label,
  value,
  format = (n) => Math.round(n).toLocaleString(),
  sub,
  tone,
  onClick,
}: {
  icon: ReactNode;
  label: string;
  value: number;
  format?: (n: number) => string;
  sub?: ReactNode;
  tone: string;
  onClick?: () => void;
}) {
  const shown = useCountUp(value);
  const content = (
    <>
      <span className="flex h-8 w-8 items-center justify-center rounded-xl text-white shadow-sm" style={{ background: tone }}>
        {icon}
      </span>
      <div className="mt-3 text-[12.5px] font-medium text-muted">{label}</div>
      <div className="font-display text-[28px] font-bold leading-tight tabular">{format(shown)}</div>
      {sub && <div className="mt-0.5 truncate text-[11.5px] text-muted">{sub}</div>}
    </>
  );
  const cls = "rounded-2xl p-4 ring-1 ring-line transition-transform duration-200 hover:-translate-y-0.5";
  if (onClick) {
    return (
      <button type="button" onClick={onClick} className={`${cls} w-full cursor-pointer text-left`} style={{ background: tint(tone) }}>
        {content}
      </button>
    );
  }
  return (
    <div className={cls} style={{ background: tint(tone) }}>
      {content}
    </div>
  );
}

type Bucket = { start: number; secs: number; label: string };

/**
 * Listening split into days, weeks or months across `from..to` (unix seconds), depending on how
 * long the period is. `byDay` holds local midnights.
 */
export function buckets(byDay: [number, number][], from: number, to: number): { unit: string; list: Bucket[] } {
  const startDay = new Date(from * 1000);
  startDay.setHours(0, 0, 0, 0);
  const end = new Date(to * 1000);
  const days = Math.max(1, Math.round((end.getTime() - startDay.getTime()) / 86_400_000));
  const list: Bucket[] = [];
  let unit: string;
  if (days <= 45) {
    unit = "day";
    for (const d = new Date(startDay); d < end; d.setDate(d.getDate() + 1)) {
      list.push({ start: d.getTime() / 1000, secs: 0, label: d.toLocaleDateString(undefined, { day: "numeric", month: "short" }) });
    }
  } else if (days <= 400) {
    unit = "week";
    // Weeks ending today.
    const today = new Date(end);
    today.setHours(0, 0, 0, 0);
    const first = new Date(today);
    first.setDate(first.getDate() - 7 * (Math.ceil(days / 7) - 1) - 6);
    for (const d = new Date(first); d < end; d.setDate(d.getDate() + 7)) {
      list.push({ start: d.getTime() / 1000, secs: 0, label: d.toLocaleDateString(undefined, { day: "numeric", month: "short" }) });
    }
  } else {
    unit = "month";
    for (const d = new Date(startDay.getFullYear(), startDay.getMonth(), 1); d < end; d.setMonth(d.getMonth() + 1)) {
      list.push({ start: d.getTime() / 1000, secs: 0, label: d.toLocaleDateString(undefined, { month: "short", year: "2-digit" }) });
    }
  }
  for (const [day, secs] of byDay) {
    // The last bucket that starts on or before this day.
    let i = list.length - 1;
    while (i > 0 && list[i].start > day + 3600) i--;
    if (list[i] && day + 3600 >= list[i].start) list[i].secs += secs;
  }
  return { unit, list };
}

/** Smooth line through points, kept inside 0..h so it never dips below the axis. */
function smooth(pts: [number, number][], h: number) {
  if (pts.length === 0) return "";
  if (pts.length === 1) return `M${pts[0][0]},${pts[0][1]}`;
  const clamp = (y: number) => Math.min(h, Math.max(0, y));
  let d = `M${pts[0][0]},${pts[0][1]}`;
  for (let i = 0; i < pts.length - 1; i++) {
    const p0 = pts[i - 1] ?? pts[i];
    const p1 = pts[i];
    const p2 = pts[i + 1];
    const p3 = pts[i + 2] ?? p2;
    const c1 = [p1[0] + (p2[0] - p0[0]) / 6, clamp(p1[1] + (p2[1] - p0[1]) / 6)];
    const c2 = [p2[0] - (p3[0] - p1[0]) / 6, clamp(p2[1] - (p3[1] - p1[1]) / 6)];
    d += ` C${c1[0]},${c1[1]} ${c2[0]},${c2[1]} ${p2[0]},${p2[1]}`;
  }
  return d;
}

/** Area chart of listening over the period, drawn in from the left. Hover shows each point. */
export function OverTime({ list, unit }: { list: Bucket[]; unit: string }) {
  const [hover, setHover] = useState<number | null>(null);
  const W = 600;
  const H = 160;
  const max = Math.max(...list.map((b) => b.secs), 60);
  const x = (i: number) => (list.length === 1 ? W / 2 : (i / (list.length - 1)) * W);
  const y = (v: number) => H - (v / max) * (H - 8);
  const pts = list.map((b, i) => [x(i), y(b.secs)] as [number, number]);
  const line = smooth(pts, H);
  const area = pts.length ? `${line} L${pts[pts.length - 1][0]},${H} L${pts[0][0]},${H} Z` : "";
  const labelEvery = Math.max(1, Math.ceil(list.length / 6));
  const move = (e: React.MouseEvent<HTMLDivElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    const i = Math.round(((e.clientX - r.left) / r.width) * (list.length - 1));
    setHover(Math.max(0, Math.min(list.length - 1, i)));
  };
  const h = hover != null ? list[hover] : null;
  return (
    <div className="flex gap-2">
      <div className="flex h-40 w-12 shrink-0 flex-col justify-between text-right text-[10.5px] text-faint tabular">
        <span>{hm(max)}</span>
        <span>{hm(max / 2)}</span>
        <span>0m</span>
      </div>
      <div className="min-w-0 flex-1">
        <div className="relative h-40" onMouseMove={move} onMouseLeave={() => setHover(null)}>
          <div className="pointer-events-none absolute inset-x-0 top-1/2 border-t border-dashed border-line" />
          <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" className="reveal-x absolute inset-0 h-full w-full overflow-visible">
            <defs>
              <linearGradient id="overtime-fill" x1="0" y1="0" x2="0" y2="1">
                <stop offset="0%" stopColor="var(--c-accent)" stopOpacity="0.35" />
                <stop offset="100%" stopColor="var(--c-accent)" stopOpacity="0.03" />
              </linearGradient>
            </defs>
            <path d={area} fill="url(#overtime-fill)" />
            <path d={line} fill="none" stroke="var(--c-accent)" strokeWidth="2.2" vectorEffect="non-scaling-stroke" strokeLinejoin="round" />
          </svg>
          {h && hover != null && (
            <>
              <div className="pointer-events-none absolute inset-y-0 w-px bg-fg/15" style={{ left: `${(x(hover) / W) * 100}%` }} />
              <div
                className="pointer-events-none absolute h-3 w-3 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-elev bg-accent shadow"
                style={{ left: `${(x(hover) / W) * 100}%`, top: `${(y(h.secs) / H) * 100}%` }}
              />
              <div
                className="pointer-events-none absolute -top-2 z-10 -translate-x-1/2 -translate-y-full whitespace-nowrap rounded-lg bg-fg px-2 py-1 text-[11.5px] font-medium text-bg shadow-lg"
                style={{ left: `${Math.min(90, Math.max(10, (x(hover) / W) * 100))}%` }}
              >
                {unit === "week" ? `Week of ${h.label}` : h.label} · {hm(h.secs)}
              </div>
            </>
          )}
        </div>
        <div className="relative mt-1.5 h-4 text-[10.5px] text-faint">
          {list.map((b, i) =>
            i % labelEvery === 0 ? (
              <span key={i} className="absolute -translate-x-1/2 whitespace-nowrap" style={{ left: `${(x(i) / W) * 100}%` }}>
                {b.label}
              </span>
            ) : null,
          )}
        </div>
      </div>
    </div>
  );
}

/** The 24 hours as rays around a clock, 00:00 at the top, with the peak hour in the middle. */
export function HourClock({ byHour }: { byHour: number[] }) {
  const max = Math.max(...byHour, 1);
  const peak = byHour.indexOf(Math.max(...byHour));
  const C = 110;
  const inner = 40;
  const reach = 58;
  return (
    <div className="flex justify-center">
      <svg viewBox="0 0 220 220" className="h-56 w-56 overflow-visible">
        <circle cx={C} cy={C} r={inner - 8} fill={tint("var(--c-accent)", 14)} />
        {byHour.map((v, h) => {
          const a = (h / 24) * Math.PI * 2 - Math.PI / 2;
          const len = Math.max(1, (v / max) * reach);
          const [cos, sin] = [Math.cos(a), Math.sin(a)];
          return (
            <line
              key={h}
              x1={C + cos * inner}
              y1={C + sin * inner}
              x2={C + cos * (inner + len)}
              y2={C + sin * (inner + len)}
              pathLength={1}
              stroke={v > 0 ? (h === peak ? "var(--c-accent)" : TONES[h % TONES.length]) : "var(--c-line)"}
              strokeOpacity={h === peak || v === 0 ? 1 : 0.75}
              strokeWidth={8}
              strokeLinecap="round"
              className="ray-out"
              style={{ animationDelay: `${h * 25}ms` }}
            >
              <title>{`${String(h).padStart(2, "0")}:00–${String((h + 1) % 24).padStart(2, "0")}:00 · ${hm(v)}`}</title>
            </line>
          );
        })}
        {[0, 6, 12, 18].map((h) => {
          const a = (h / 24) * Math.PI * 2 - Math.PI / 2;
          return (
            <text key={h} x={C + Math.cos(a) * 106} y={C + Math.sin(a) * 106} textAnchor="middle" dominantBaseline="middle" className="fill-faint text-[10px]">
              {String(h).padStart(2, "0")}
            </text>
          );
        })}
        <text x={C} y={C - 4} textAnchor="middle" className="fill-fg font-display text-[17px] font-bold">
          {String(peak).padStart(2, "0")}:00
        </text>
        <text x={C} y={C + 12} textAnchor="middle" className="fill-muted text-[8.5px]">
          your peak hour
        </text>
      </svg>
    </div>
  );
}

/** Genres as one stacked bar, then the biggest five with their share. */
export function GenreMix({ genres }: { genres: [string, number][] }) {
  const total = genres.reduce((n, [, s]) => n + s, 0) || 1;
  const top = genres.slice(0, 5);
  const rest = genres.slice(5).reduce((n, [, s]) => n + s, 0);
  const rows: [string, number, string][] = top.map(([g, s], i) => [g, s, TONES[i % TONES.length]]);
  if (rest > 0) rows.push(["Other", rest, "var(--c-faint)"]);
  return (
    <div>
      <div className="grow-x flex h-3 gap-1">
        {rows.map(([g, s, c]) => (
          <div key={g} title={g} className="h-full rounded-full" style={{ flexGrow: s, background: c }} />
        ))}
      </div>
      <ul className="mt-4 space-y-2.5">
        {rows.map(([g, s, c], i) => (
          <li key={g} className="flex items-center gap-3 text-[13px]">
            <span className="h-2.5 w-2.5 shrink-0 rounded-full" style={{ background: c }} />
            <span className="w-24 shrink-0 truncate font-medium">{g}</span>
            <span className="h-1.5 flex-1 overflow-hidden rounded-full bg-fg/10">
              <span
                className="grow-x block h-full rounded-full"
                style={{ width: `${(s / (rows[0][1] || 1)) * 100}%`, background: c, animationDelay: `${150 + i * 70}ms` }}
              />
            </span>
            <span className="w-10 shrink-0 text-right text-[12px] text-muted tabular">{Math.round((s / total) * 100)}%</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
