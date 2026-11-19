import { Heart } from "lucide-react";
import { useEffect, useRef, useState, type CSSProperties } from "react";
import { useIsLoved, useLoved } from "../store/loved";

/** Six small sparks radiating from the heart when a song is loved. */
const SPARKS: [number, number][] = [
  [0, -14],
  [12, -7],
  [12, 7],
  [0, 14],
  [-12, 7],
  [-12, -7],
];

export function HeartButton({
  path,
  size = 15,
  shortcut = false,
  className = "",
}: {
  path: string;
  size?: number;
  /** Mention Ctrl+L in the tooltip (for the playing song). */
  shortcut?: boolean;
  className?: string;
}) {
  const loved = useIsLoved(path);
  const [burst, setBurst] = useState(0);
  const prev = useRef({ path, loved });

  useEffect(() => {
    // Only pop when this same song transitions from unloved to loved.
    if (prev.current.path === path && !prev.current.loved && loved) {
      setBurst((n) => n + 1);
    } else if (prev.current.path !== path || !loved) {
      setBurst(0);
    }
    prev.current = { path, loved };
  }, [path, loved]);

  const label = loved
    ? shortcut
      ? "Unlove (Ctrl+L)"
      : "Unlove"
    : shortcut
      ? "Love (Ctrl+L)"
      : "Love";

  return (
    <button
      type="button"
      aria-label={label}
      aria-pressed={loved}
      title={label}
      onClick={(e) => {
        e.stopPropagation();
        useLoved.getState().toggle(path);
      }}
      onDoubleClick={(e) => e.stopPropagation()}
      className={`relative inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-full transition-colors hover:bg-hover ${
        loved ? "text-accent" : "text-muted hover:text-fg"
      } ${className}`}
    >
      <Heart
        key={burst}
        size={size}
        fill={loved ? "currentColor" : "none"}
        className={burst > 0 && loved ? "heart-pop" : undefined}
      />
      {burst > 0 && loved && (
        <span key={`s-${burst}`} aria-hidden className="pointer-events-none absolute inset-0">
          {SPARKS.map(([tx, ty], i) => (
            <span
              key={i}
              className={`heart-spark absolute left-1/2 top-1/2 h-1 w-1 rounded-full ${i % 2 === 0 ? "bg-accent" : "bg-accent2"}`}
              style={{ ["--tx"]: `${tx}px`, ["--ty"]: `${ty}px` } as CSSProperties}
            />
          ))}
        </span>
      )}
    </button>
  );
}
