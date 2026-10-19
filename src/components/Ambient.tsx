/**
 * Spotify-style ambient backdrop: a soft wash of the artwork's color at the top of the
 * content area. The color cross-fades when it changes.
 */
export function Ambient({ color }: { color: string | null }) {
  return (
    <div
      aria-hidden
      className="pointer-events-none absolute inset-x-0 top-0 h-[460px] opacity-45 transition-[background-color] duration-[900ms] ease-out dark:opacity-100"
      style={{
        backgroundColor: color ?? "transparent",
        maskImage: "linear-gradient(to bottom, rgba(0,0,0,0.5) 0%, rgba(0,0,0,0.2) 45%, transparent 100%)",
        WebkitMaskImage: "linear-gradient(to bottom, rgba(0,0,0,0.5) 0%, rgba(0,0,0,0.2) 45%, transparent 100%)",
      }}
    />
  );
}
