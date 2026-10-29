/**
 * A small music note flies from the button that was clicked to the Downloads button in the
 * sidebar, which then gives a little bump, so it's clear the songs were queued. Up to three
 * notes for several songs. Nothing moves when Windows asks for less motion or the sidebar is
 * hidden (say, behind Now Playing).
 */
export function flyToDownloads(from: DOMRect | null | undefined, count = 1) {
  const target = document.querySelector<HTMLElement>("[data-downloads-target]");
  if (!from || !target || count < 1 || matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  const to = target.getBoundingClientRect();
  if (!to.width || !to.height) return;

  const size = 26;
  const x0 = from.left + from.width / 2 - size / 2;
  const y0 = from.top + from.height / 2 - size / 2;
  const x1 = to.left + to.width / 2 - size / 2;
  const y1 = to.top + to.height / 2 - size / 2;
  // Arc upwards, so the note doesn't just slide in a straight line.
  const lift = Math.min(140, 40 + Math.abs(x1 - x0) * 0.15);
  const xm = (x0 + x1) / 2;
  const ym = Math.min(y0, y1) - lift;

  const notes = Math.min(count, 3);
  for (let i = 0; i < notes; i++) {
    const el = document.createElement("div");
    el.textContent = "♪";
    el.setAttribute("aria-hidden", "true");
    Object.assign(el.style, {
      position: "fixed",
      left: "0",
      top: "0",
      width: `${size}px`,
      height: `${size}px`,
      zIndex: "9999",
      pointerEvents: "none",
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      borderRadius: "9999px",
      fontSize: "15px",
      fontWeight: "700",
      color: "var(--c-on-accent)",
      background: "linear-gradient(135deg, var(--c-accent), var(--c-accent2, var(--c-accent)))",
      boxShadow: "0 4px 14px rgba(0, 0, 0, 0.35)",
    } satisfies Partial<CSSStyleDeclaration>);
    document.body.appendChild(el);
    const anim = el.animate(
      [
        { transform: `translate(${x0}px, ${y0}px) scale(0.6)`, opacity: 0 },
        { transform: `translate(${x0}px, ${y0 - 8}px) scale(1.1)`, opacity: 1, offset: 0.12 },
        { transform: `translate(${xm}px, ${ym}px) scale(1)`, opacity: 1, offset: 0.55 },
        { transform: `translate(${x1}px, ${y1}px) scale(0.45)`, opacity: 0.2 },
      ],
      { duration: 750, delay: i * 110, easing: "cubic-bezier(0.45, 0, 0.25, 1)", fill: "both" },
    );
    anim.onfinish = anim.oncancel = () => el.remove();
    if (i === notes - 1) {
      anim.addEventListener("finish", () =>
        target.animate(
          [{ transform: "scale(1)" }, { transform: "scale(1.25)" }, { transform: "scale(1)" }],
          { duration: 320, easing: "ease-out" },
        ),
      );
    }
  }
}
