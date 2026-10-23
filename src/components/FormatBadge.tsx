import { useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { usePlayer } from "../store/player";
import { fmtOutput, fmtRate, fmtSource } from "../lib/format";

type Tone = "ok" | "warn" | "muted";

/** Live signal-path info: source format -> output format, with bit-perfect indicator. */
export function FormatBadge({ detailed = false }: { detailed?: boolean }) {
  const status = usePlayer((s) => s.status);
  if (!status?.source) return null;
  const { source, output, bitPerfect, resampled, eqActive, replayGainDb } = status;

  /** `short` is shown instead of `label` when the compact line is narrow; `tip` explains the tag on hover. */
  const tags: { label: string; short?: string; tone: Tone; tip: string }[] = [];
  if (bitPerfect)
    tags.push({
      label: "Bit-perfect",
      tone: "ok",
      tip: "The file reaches your device exactly as it is: no resampling, EQ or volume change.",
    });
  if (resampled && output)
    tags.push({
      label: `Resampled → ${fmtRate(output.sampleRate)}`,
      short: `→ ${fmtRate(output.sampleRate)}`,
      tone: "warn",
      tip:
        `Windows runs this device at ${fmtRate(output.sampleRate)} and the song is ${fmtRate(source.sampleRate)}, so Undertone converts it before playing. ` +
        "This doesn't add quality, and with Undertone's converter you won't hear a difference. " +
        "To play files at their own rate, choose Exclusive in Settings → Audio output, or change the device's format in Windows sound settings.",
    });
  if (eqActive) tags.push({ label: "EQ", tone: "muted", tip: "The equalizer is changing the sound." });
  if (replayGainDb != null && Math.abs(replayGainDb) > 0.01) {
    const db = `${replayGainDb > 0 ? "+" : ""}${replayGainDb.toFixed(1)} dB`;
    tags.push({ label: `RG ${db}`, tone: "muted", tip: `ReplayGain changes this song's volume by ${db} so songs play at a similar loudness.` });
  }

  const toneClass = (t: Tone) =>
    t === "ok" ? "text-ok ring-ok/40 bg-ok/10" : t === "warn" ? "text-warn ring-warn/40 bg-warn/10" : "text-muted ring-line";

  if (!detailed) {
    // The format text wins over the tag: in a narrow line the tag shortens before the text is cut.
    return (
      <div className="@container min-w-0">
        <div className="flex min-w-0 items-center gap-1.5 text-[11px]">
          <span className="truncate whitespace-nowrap text-muted" title={`${fmtSource(source)}\n${fmtOutput(output)}${output ? ` · ${output.deviceName}` : ""}`}>
            {fmtSource(source)}
          </span>
          {tags.slice(0, 1).map((t) => (
            <Tip key={t.label} text={t.tip}>
              <span className={`shrink-0 cursor-help whitespace-nowrap rounded px-1.5 py-px font-medium ring-1 ${toneClass(t.tone)}`}>
                {t.short ? (
                  <>
                    <span className="@max-[22rem]:hidden">{t.label}</span>
                    <span className="hidden @max-[22rem]:inline">{t.short}</span>
                  </>
                ) : (
                  t.label
                )}
              </span>
            </Tip>
          ))}
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-2 text-[12.5px]">
      <Row label="Source" value={fmtSource(source)} />
      <Row label="Output" value={fmtOutput(output)} />
      <Row label="Device" value={output?.deviceName ?? "—"} />
      {tags.length > 0 && (
        <div className="flex flex-wrap gap-1.5 pt-1">
          {tags.map((t) => (
            <Tip key={t.label} text={t.tip}>
              <span className={`cursor-help rounded px-2 py-0.5 text-[11px] font-medium ring-1 ${toneClass(t.tone)}`}>{t.label}</span>
            </Tip>
          ))}
        </div>
      )}
    </div>
  );
}

/**
 * A short explanation shown above (or below, near the top of the window) its child on hover or
 * keyboard focus. Rendered into the body so containers that clip overflow can't cut it off.
 */
function Tip({ text, children }: { text: string; children: ReactNode }) {
  const ref = useRef<HTMLSpanElement>(null);
  const timer = useRef<number>();
  const [box, setBox] = useState<DOMRect | null>(null);
  const show = () => {
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => ref.current && setBox(ref.current.getBoundingClientRect()), 250);
  };
  const hide = () => {
    window.clearTimeout(timer.current);
    setBox(null);
  };

  const width = 280;
  const above = box ? box.top > 160 : true;
  const left = box ? Math.max(8, Math.min(box.left + box.width / 2 - width / 2, window.innerWidth - width - 8)) : 0;
  return (
    <span ref={ref} tabIndex={0} aria-label={text} className="inline-flex shrink-0 rounded outline-none focus-visible:ring-2 focus-visible:ring-accent" onMouseEnter={show} onMouseLeave={hide} onFocus={show} onBlur={hide}>
      {children}
      {box &&
        createPortal(
          <div
            role="tooltip"
            className="pointer-events-none fixed z-[200] rounded-lg bg-panel px-3 py-2 text-[12px] leading-relaxed text-fg shadow-xl ring-1 ring-line animate-in"
            style={{ width, left, ...(above ? { bottom: window.innerHeight - box.top + 8 } : { top: box.bottom + 8 }) }}
          >
            {text}
          </div>,
          document.body,
        )}
    </span>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex gap-3">
      <span className="w-14 shrink-0 text-faint">{label}</span>
      <span className="min-w-0 truncate text-fg/90">{value}</span>
    </div>
  );
}
