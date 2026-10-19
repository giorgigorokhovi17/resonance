import { useEffect, useRef, useState } from "react";
import { api } from "../lib/api";
import { fmtTime } from "../lib/format";
import { livePosition, usePlayer } from "../store/player";

/** Seek bar with smooth (rAF-interpolated) progress and drag-to-seek. */
export function Seekbar({ compact = false }: { compact?: boolean }) {
  const duration = usePlayer((s) => s.status?.duration ?? 0);
  const hasTrack = usePlayer((s) => !!s.status?.track);
  const [pos, setPos] = useState(0);
  const [drag, setDrag] = useState<number | null>(null);
  const [hover, setHover] = useState<number | null>(null);
  const barRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let raf = 0;
    const tick = () => {
      setPos(livePosition());
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, []);

  const fracAt = (clientX: number) => {
    const r = barRef.current!.getBoundingClientRect();
    return Math.min(1, Math.max(0, (clientX - r.left) / r.width));
  };

  const onDown = (e: React.PointerEvent) => {
    if (!hasTrack || duration <= 0) return;
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
    setDrag(fracAt(e.clientX));
  };
  const onMove = (e: React.PointerEvent) => {
    if (!barRef.current) return;
    const f = fracAt(e.clientX);
    setHover(f);
    if (drag !== null) setDrag(f);
  };
  const onUp = async (e: React.PointerEvent) => {
    if (drag === null) return;
    const f = fracAt(e.clientX);
    setDrag(null);
    const target = f * duration;
    // Optimistically jump the displayed position.
    const st = usePlayer.getState();
    if (st.status) st.setStatus({ ...st.status, position: target });
    await api.seek(target);
  };

  const shown = drag !== null ? drag * duration : pos;
  const frac = duration > 0 ? Math.min(1, shown / duration) : 0;

  return (
    <div className={`flex w-full items-center gap-3 ${compact ? "text-[11px]" : "text-[11.5px]"}`}>
      <span className="tabular w-11 text-right text-muted">{fmtTime(shown)}</span>
      <div
        ref={barRef}
        className="group relative flex h-4 flex-1 cursor-pointer items-center"
        onPointerDown={onDown}
        onPointerMove={onMove}
        onPointerUp={onUp}
        onPointerLeave={() => setHover(null)}
        role="slider"
        aria-label="Seek"
        aria-valuemin={0}
        aria-valuemax={Math.round(duration)}
        aria-valuenow={Math.round(shown)}
      >
        <div className="relative h-1 w-full overflow-hidden rounded-full bg-fg/15 transition-[height] group-hover:h-1.5">
          <div
            className={`absolute inset-y-0 left-0 rounded-full ${drag !== null ? "accent-gradient" : "bg-fg group-hover:accent-gradient"}`}
            style={{ width: `${frac * 100}%` }}
          />
        </div>
        <div
          className="pointer-events-none absolute top-1/2 h-3 w-3 -translate-x-1/2 -translate-y-1/2 rounded-full bg-fg opacity-0 shadow transition-opacity group-hover:opacity-100"
          style={{ left: `${frac * 100}%` }}
        />
        {hover !== null && duration > 0 && (
          <div
            className="pointer-events-none absolute -top-7 -translate-x-1/2 rounded bg-panel px-1.5 py-0.5 text-[11px] text-fg shadow-lg ring-1 ring-line"
            style={{ left: `${hover * 100}%` }}
          >
            {fmtTime(hover * duration)}
          </div>
        )}
      </div>
      <span className="tabular w-11 text-muted">{fmtTime(duration)}</span>
    </div>
  );
}
