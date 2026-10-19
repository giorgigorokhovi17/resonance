import { usePlayer } from "../store/player";
import { fmtOutput, fmtSource } from "../lib/format";

/** Live signal-path info: source format -> output format, with bit-perfect indicator. */
export function FormatBadge({ detailed = false }: { detailed?: boolean }) {
  const status = usePlayer((s) => s.status);
  if (!status?.source) return null;
  const { source, output, bitPerfect, resampled, eqActive, replayGainDb } = status;

  const tags: { label: string; tone: "ok" | "warn" | "muted" }[] = [];
  if (bitPerfect) tags.push({ label: "Bit-perfect", tone: "ok" });
  if (resampled && output) tags.push({ label: `Resampled → ${output.sampleRate / 1000} kHz`, tone: "warn" });
  if (eqActive) tags.push({ label: "EQ", tone: "muted" });
  if (replayGainDb != null && Math.abs(replayGainDb) > 0.01)
    tags.push({ label: `RG ${replayGainDb > 0 ? "+" : ""}${replayGainDb.toFixed(1)} dB`, tone: "muted" });

  const toneClass = (t: "ok" | "warn" | "muted") =>
    t === "ok" ? "text-ok ring-ok/40 bg-ok/10" : t === "warn" ? "text-warn ring-warn/40 bg-warn/10" : "text-muted ring-line";

  if (!detailed) {
    return (
      <div className="flex min-w-0 items-center gap-1.5 text-[11px]" title={`${fmtSource(source)}\n${fmtOutput(output)}${output ? ` · ${output.deviceName}` : ""}`}>
        <span className="truncate text-muted">{fmtSource(source)}</span>
        {tags.slice(0, 1).map((t) => (
          <span key={t.label} className={`shrink-0 rounded px-1.5 py-px font-medium ring-1 ${toneClass(t.tone)}`}>
            {t.label}
          </span>
        ))}
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
            <span key={t.label} className={`rounded px-2 py-0.5 text-[11px] font-medium ring-1 ${toneClass(t.tone)}`}>
              {t.label}
            </span>
          ))}
        </div>
      )}
    </div>
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
