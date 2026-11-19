import { useEffect, useRef, useState } from "react";

const reduced = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;

export interface GoalRingProps {
  /** Minutes listened today (may be fractional for smooth arc fill). */
  minutes: number;
  /** Daily goal in minutes (> 0). */
  goalMin: number;
  /** Diameter in px (16 for PlayerBar, 52+ for Stats). */
  size?: number;
  /** Stroke thickness in px; defaults based on size. */
  strokeWidth?: number;
  /** Incrementing signal that triggers one short glow pulse when the goal is newly met. */
  pulseKey?: number;
  className?: string;
}

/** Circular progress ring for the daily listening goal. */
export function GoalRing({ minutes, goalMin, size = 56, strokeWidth, pulseKey = 0, className = "" }: GoalRingProps) {
  const sw = strokeWidth ?? (size <= 20 ? 2.2 : 5);
  const r = (size - sw) / 2;
  const c = 2 * Math.PI * r;
  const ratio = goalMin > 0 ? Math.min(1, Math.max(0, minutes / goalMin)) : 0;
  const targetOffset = c * (1 - ratio);

  // Start empty on first mount so the stroke animates smoothly into place (skipped under reduced motion).
  const [ready, setReady] = useState(reduced);
  useEffect(() => {
    if (ready) return;
    const id = requestAnimationFrame(() => setReady(true));
    return () => cancelAnimationFrame(id);
  }, [ready]);

  const prevPulse = useRef(pulseKey);
  const [pulsing, setPulsing] = useState(false);
  useEffect(() => {
    if (pulseKey > 0 && pulseKey !== prevPulse.current) {
      prevPulse.current = pulseKey;
      setPulsing(true);
      const id = window.setTimeout(() => setPulsing(false), 1200);
      return () => window.clearTimeout(id);
    }
    prevPulse.current = pulseKey;
  }, [pulseKey]);

  const offset = ready ? targetOffset : c;
  const center = size / 2;
  const showLabel = size >= 40;

  return (
    <span
      className={`relative inline-flex shrink-0 items-center justify-center ${pulsing ? "goal-ring-pulse" : ""} ${className}`}
      style={{ width: size, height: size }}
    >
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} aria-hidden="true" className="block overflow-visible">
        <circle
          cx={center}
          cy={center}
          r={r}
          fill="none"
          stroke="var(--c-faint)"
          strokeOpacity={0.3}
          strokeWidth={sw}
        />
        <circle
          cx={center}
          cy={center}
          r={r}
          fill="none"
          stroke="var(--c-accent)"
          strokeWidth={sw}
          strokeLinecap="round"
          strokeDasharray={c}
          strokeDashoffset={offset}
          opacity={ratio > 0 ? 1 : 0}
          transform={`rotate(-90 ${center} ${center})`}
          className="goal-ring-arc"
        />
      </svg>
      {showLabel && (
        <span className="tabular pointer-events-none absolute inset-0 flex flex-col items-center justify-center leading-none">
          <span className="text-[13px] font-semibold text-fg">{Math.floor(minutes)}</span>
          <span className="mt-0.5 text-[9.5px] text-muted">min</span>
        </span>
      )}
    </span>
  );
}
