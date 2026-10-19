import { open } from "@tauri-apps/plugin-dialog";
import { FileInput, RotateCcw, Save, Trash2 } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { confirmDialog, promptText } from "../components/Dialog";
import { SecondaryButton } from "../components/EmptyState";
import { api } from "../lib/api";
import { BUILTIN_PRESETS, bandActive, curve, effectivePreamp, flatEq, maxResponseDb } from "../lib/eqmath";
import type { EqBand, EqPreset, EqSettings, FilterType } from "../lib/types";
import { usePlayer } from "../store/player";
import { HeadphoneFinder } from "./eq/HeadphoneFinder";
import { Toggle } from "./SettingsView";

const FILTERS: { value: FilterType; label: string; short: string }[] = [
  { value: "peaking", label: "Peak", short: "PK" },
  { value: "lowShelf", label: "Low shelf", short: "LS" },
  { value: "highShelf", label: "High shelf", short: "HS" },
  { value: "lowPass", label: "Low-pass", short: "LP" },
  { value: "highPass", label: "High-pass", short: "HP" },
];

const fmtFreq = (f: number) => (f >= 1000 ? `${+(f / 1000).toFixed(f >= 10000 ? 1 : 2)}k` : `${Math.round(f)}`);
const fmtDb = (v: number) => `${v > 0 ? "+" : ""}${v.toFixed(1)}`;
const toastErr = (e: unknown) => usePlayer.getState().pushToast("error", String(e));

/** Sends EQ changes to the engine at most every ~40 ms while dragging. */
function useThrottledApply() {
  const pending = useRef<EqSettings | null>(null);
  const timer = useRef<number | null>(null);
  return useCallback((eq: EqSettings) => {
    pending.current = eq;
    if (timer.current !== null) return;
    timer.current = window.setTimeout(async () => {
      timer.current = null;
      const e = pending.current;
      pending.current = null;
      if (e) await api.setEq(e).catch(toastErr);
    }, 40);
  }, []);
}

function ResponseGraph({ eq, onBand }: { eq: EqSettings; onBand: (i: number, patch: Partial<EqBand>) => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [w, setW] = useState(800);
  const h = 260;
  const pad = { l: 40, r: 12, t: 12, b: 22 };
  useLayoutEffect(() => {
    const ro = new ResizeObserver(() => ref.current && setW(ref.current.clientWidth));
    if (ref.current) ro.observe(ref.current);
    return () => ro.disconnect();
  }, []);

  const pts = useMemo(() => curve(eq, 260), [eq]);
  const maxAbs = Math.max(
    0,
    ...pts.map((p) => Math.abs(p.db)),
    ...eq.bands.filter((b) => b.enabled).map((b) => Math.abs(b.gain)),
  );
  const range = Math.min(30, Math.max(12, Math.ceil((maxAbs + 1) / 6) * 6));
  const x = (f: number) => pad.l + ((Math.log10(f) - Math.log10(20)) / 3) * (w - pad.l - pad.r);
  const y = (db: number) => pad.t + ((range - db) / (2 * range)) * (h - pad.t - pad.b);
  const fAt = (px: number) => Math.pow(10, Math.log10(20) + ((px - pad.l) / (w - pad.l - pad.r)) * 3);
  const dbAt = (py: number) => range - ((py - pad.t) / (h - pad.t - pad.b)) * 2 * range;

  const path = pts.map((p, i) => `${i ? "L" : "M"}${x(p.f).toFixed(1)},${y(Math.max(-range, Math.min(range, p.db))).toFixed(1)}`).join("");
  const area = `${path}L${x(20000)},${y(0)}L${x(20)},${y(0)}Z`;
  const drag = useRef<number | null>(null);

  const onPointerMove = (e: React.PointerEvent) => {
    if (drag.current === null) return;
    const r = ref.current!.getBoundingClientRect();
    const i = drag.current;
    const b = eq.bands[i];
    const freq = Math.round(Math.max(20, Math.min(20000, fAt(e.clientX - r.left))));
    const patch: Partial<EqBand> = { freq };
    if (b.filter !== "lowPass" && b.filter !== "highPass") {
      patch.gain = Math.round(Math.max(-20, Math.min(20, dbAt(e.clientY - r.top))) * 10) / 10;
    }
    onBand(i, patch);
  };

  const gridF = [20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000];
  const gridDb = [];
  for (let d = -range; d <= range; d += range > 12 ? 6 : 3) gridDb.push(d);

  return (
    <div
      ref={ref}
      className="relative select-none"
      onPointerMove={onPointerMove}
      onPointerUp={() => (drag.current = null)}
      onPointerLeave={() => (drag.current = null)}
    >
      <svg width={w} height={h} className="block">
        <defs>
          <linearGradient id="eqfill" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" stopColor="var(--c-accent)" stopOpacity="0.28" />
            <stop offset="1" stopColor="var(--c-accent2)" stopOpacity="0.02" />
          </linearGradient>
          <linearGradient id="eqline" x1="0" y1="0" x2="1" y2="0">
            <stop offset="0" stopColor="var(--c-accent)" />
            <stop offset="1" stopColor="var(--c-accent2)" />
          </linearGradient>
        </defs>
        {gridF.map((f) => (
          <g key={f}>
            <line x1={x(f)} x2={x(f)} y1={pad.t} y2={h - pad.b} stroke="var(--c-line)" strokeWidth={1} />
            <text x={x(f)} y={h - 6} fontSize={10} textAnchor="middle" fill="var(--c-faint)">
              {fmtFreq(f)}
            </text>
          </g>
        ))}
        {gridDb.map((d) => (
          <g key={d}>
            <line
              x1={pad.l}
              x2={w - pad.r}
              y1={y(d)}
              y2={y(d)}
              stroke={d === 0 ? "var(--c-muted)" : "var(--c-line)"}
              strokeWidth={d === 0 ? 1 : 1}
              strokeDasharray={d === 0 ? "4 3" : undefined}
            />
            <text x={pad.l - 6} y={y(d) + 3} fontSize={10} textAnchor="end" fill="var(--c-faint)">
              {d > 0 ? `+${d}` : d}
            </text>
          </g>
        ))}
        <path d={area} fill="url(#eqfill)" opacity={eq.enabled ? 1 : 0.3} />
        <path d={path} fill="none" stroke="url(#eqline)" strokeWidth={2.25} opacity={eq.enabled ? 1 : 0.35} />
        {eq.bands.map((b, i) =>
          b.enabled ? (
            <g key={i} style={{ cursor: "grab" }}>
              <circle
                cx={x(Math.max(20, Math.min(20000, b.freq)))}
                cy={y(b.filter === "lowPass" || b.filter === "highPass" ? 0 : Math.max(-range, Math.min(range, b.gain)))}
                r={9}
                fill="var(--c-panel)"
                stroke={bandActive(b) ? "var(--c-accent)" : "var(--c-faint)"}
                strokeWidth={2}
                onPointerDown={(e) => {
                  (e.target as Element).setPointerCapture(e.pointerId);
                  drag.current = i;
                }}
                onWheel={(e) => onBand(i, { q: Math.max(0.1, Math.min(10, +(b.q * (e.deltaY < 0 ? 1.1 : 0.9)).toFixed(2))) })}
              >
                <title>{`Band ${i + 1}: ${fmtFreq(b.freq)} Hz, ${fmtDb(b.gain)} dB, Q ${b.q.toFixed(2)} (drag to move, scroll for Q)`}</title>
              </circle>
              <text
                x={x(Math.max(20, Math.min(20000, b.freq)))}
                y={y(b.filter === "lowPass" || b.filter === "highPass" ? 0 : Math.max(-range, Math.min(range, b.gain))) + 3.5}
                fontSize={10}
                fontWeight={600}
                textAnchor="middle"
                fill="var(--c-fg)"
                pointerEvents="none"
              >
                {i + 1}
              </text>
            </g>
          ) : null,
        )}
      </svg>
    </div>
  );
}

function NumField({
  value,
  onChange,
  step = 0.1,
  min,
  max,
  suffix,
  width = "w-16",
  disabled,
}: {
  value: number;
  onChange: (v: number) => void;
  step?: number;
  min?: number;
  max?: number;
  suffix?: string;
  width?: string;
  disabled?: boolean;
}) {
  const [text, setText] = useState(String(value));
  useEffect(() => setText(String(value)), [value]);
  const commit = () => {
    const v = parseFloat(text.replace(",", "."));
    if (isFinite(v)) onChange(Math.min(max ?? Infinity, Math.max(min ?? -Infinity, v)));
    else setText(String(value));
  };
  return (
    <label className={`flex items-center rounded bg-bg ring-1 ring-line focus-within:ring-accent ${width} ${disabled ? "opacity-40" : ""}`}>
      <input
        value={text}
        disabled={disabled}
        onChange={(e) => setText(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") (e.target as HTMLInputElement).blur();
          if (e.key === "ArrowUp" || e.key === "ArrowDown") {
            e.preventDefault();
            onChange(Math.min(max ?? Infinity, Math.max(min ?? -Infinity, +(value + (e.key === "ArrowUp" ? step : -step)).toFixed(2))));
          }
        }}
        className="tabular w-full min-w-0 bg-transparent px-1.5 py-1 text-center text-[12px] outline-none"
      />
      {suffix && <span className="pr-1.5 text-[10px] text-faint">{suffix}</span>}
    </label>
  );
}

function BandStrip({ band, index, onChange }: { band: EqBand; index: number; onChange: (p: Partial<EqBand>) => void }) {
  const gainless = band.filter === "lowPass" || band.filter === "highPass";
  return (
    <div className={`flex flex-col items-center gap-2 rounded-lg bg-bg/60 px-1.5 py-3 ring-1 ring-line ${band.enabled ? "" : "opacity-50"}`}>
      <div className="flex w-full items-center justify-between px-0.5">
        <span className="text-[11px] font-semibold text-muted">{index + 1}</span>
        <input
          type="checkbox"
          checked={band.enabled}
          onChange={(e) => onChange({ enabled: e.target.checked })}
          aria-label={`Enable band ${index + 1}`}
          className="h-3.5 w-3.5 accent-[var(--c-accent)]"
        />
      </div>
      <select
        value={band.filter}
        onChange={(e) => onChange({ filter: e.target.value as FilterType })}
        aria-label={`Band ${index + 1} type`}
        className="w-full rounded bg-bg py-0.5 text-center text-[11px] outline-none ring-1 ring-line"
      >
        {FILTERS.map((f) => (
          <option key={f.value} value={f.value}>
            {f.short}
          </option>
        ))}
      </select>
      <div className="flex h-36 items-center">
        <input
          type="range"
          min={-15}
          max={15}
          step={0.1}
          value={Math.max(-15, Math.min(15, band.gain))}
          disabled={gainless || !band.enabled}
          aria-label={`Band ${index + 1} gain`}
          onChange={(e) => onChange({ gain: parseFloat(e.target.value) })}
          onDoubleClick={() => onChange({ gain: 0 })}
          className="h-36 w-5 cursor-pointer accent-[var(--c-accent)] disabled:opacity-30"
          style={{ writingMode: "vertical-lr", direction: "rtl" }}
        />
      </div>
      <NumField value={+band.gain.toFixed(1)} onChange={(gain) => onChange({ gain })} min={-30} max={30} suffix="dB" width="w-full" disabled={gainless} />
      <NumField value={Math.round(band.freq)} onChange={(freq) => onChange({ freq })} min={10} max={24000} step={10} suffix="Hz" width="w-full" />
      <NumField value={+band.q.toFixed(2)} onChange={(q) => onChange({ q })} min={0.05} max={20} step={0.05} suffix="Q" width="w-full" />
    </div>
  );
}

export function EqView() {
  const settings = usePlayer((s) => s.settings);
  const setSettings = usePlayer((s) => s.setSettings);
  const [presets, setPresets] = useState<EqPreset[]>([]);
  const apply = useThrottledApply();

  useEffect(() => {
    api.getEqPresets().then(setPresets).catch(toastErr);
  }, []);

  const eq = settings?.eq;
  const update = useCallback(
    (next: EqSettings) => {
      const st = usePlayer.getState().settings;
      if (!st) return;
      setSettings({ ...st, eq: next });
      apply(next);
    },
    [apply, setSettings],
  );

  if (!eq || !settings) return null;
  const effPre = effectivePreamp(eq);
  const peak = maxResponseDb(eq);

  const setBand = (i: number, patch: Partial<EqBand>) => {
    const bands = eq.bands.map((b, k) => (k === i ? { ...b, ...patch } : b));
    update({ ...eq, bands, presetName: null });
  };

  const loadPreset = (name: string) => {
    const builtin = BUILTIN_PRESETS.find((p) => p.presetName === name);
    const saved = presets.find((p) => p.name === name);
    const src = saved?.eq ?? builtin;
    if (!src) return;
    update({ ...src, enabled: true, presetName: name });
  };

  const savePreset = async () => {
    const name = await promptText("Save EQ preset", eq.presetName ?? "", "Save");
    if (!name) return;
    try {
      setPresets(await api.saveEqPreset(name, eq));
      update({ ...eq, presetName: name });
      usePlayer.getState().pushToast("info", `Saved preset "${name}".`);
    } catch (e) {
      toastErr(e);
    }
  };

  const deletePreset = async () => {
    const name = eq.presetName;
    if (!name || !presets.some((p) => p.name === name)) return;
    if (!(await confirmDialog(`Delete preset "${name}"?`, "The current EQ settings stay active."))) return;
    setPresets(await api.deleteEqPreset(name));
    update({ ...eq, presetName: null });
  };

  const importAutoEq = async () => {
    const picked = await open({
      multiple: false,
      title: "Import AutoEQ ParametricEQ.txt",
      filters: [{ name: "AutoEQ / Equalizer APO", extensions: ["txt"] }],
    });
    if (!picked || Array.isArray(picked)) return;
    try {
      const r = await api.importAutoEq(picked);
      update(r.eq);
      setPresets(await api.saveEqPreset(r.name, r.eq));
      usePlayer
        .getState()
        .pushToast(
          r.warnings.length ? "error" : "info",
          `Imported "${r.name}" and saved it as a preset.${r.warnings.length ? " " + r.warnings.join(" ") : ""}`,
        );
    } catch (e) {
      toastErr(e);
    }
  };

  const applyHeadphoneEq = async (next: EqSettings) => {
    update(next);
    try {
      setPresets(await api.saveEqPreset(next.presetName!, next));
      usePlayer.getState().pushToast("info", `Applied "${next.presetName}" and saved it as a preset.`);
    } catch (e) {
      toastErr(e);
    }
  };

  const isSaved = !!eq.presetName && presets.some((p) => p.name === eq.presetName);

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-6xl space-y-5 px-8 py-6">
        <div className="flex flex-wrap items-end justify-between gap-4">
          <div>
            <h1 className="font-display text-[28px] font-semibold">Equalizer</h1>
            <p className="mt-1 text-[12.5px] text-muted">
              10-band parametric EQ, processed in 64-bit float in both output modes.{" "}
              {eq.enabled ? "The signal is not bit-perfect while the EQ is active." : "Bypassed: the signal path stays bit-perfect."}
            </p>
          </div>
          <label className="flex items-center gap-3 rounded-full bg-panel px-4 py-2 ring-1 ring-line">
            <span className="text-[13px] font-medium">{eq.enabled ? "EQ on" : "Bypassed"}</span>
            <Toggle label="Enable equalizer" checked={eq.enabled} onChange={(v) => update({ ...eq, enabled: v })} />
          </label>
        </div>

        <HeadphoneFinder onApply={applyHeadphoneEq} />

        <div className="flex flex-wrap items-center gap-2">
          <select
            value={eq.presetName ?? ""}
            onChange={(e) => loadPreset(e.target.value)}
            aria-label="Preset"
            className="h-9 min-w-[220px] rounded-full bg-panel px-4 text-[13px] outline-none ring-1 ring-line focus:ring-accent"
          >
            <option value="" disabled>
              Custom
            </option>
            <optgroup label="Built-in">
              {BUILTIN_PRESETS.map((p) => (
                <option key={p.presetName!} value={p.presetName!}>
                  {p.presetName}
                </option>
              ))}
            </optgroup>
            {presets.length > 0 && (
              <optgroup label="Saved">
                {presets.map((p) => (
                  <option key={p.name} value={p.name}>
                    {p.name}
                  </option>
                ))}
              </optgroup>
            )}
          </select>
          <SecondaryButton onClick={savePreset}>
            <Save size={15} /> Save preset
          </SecondaryButton>
          <SecondaryButton onClick={importAutoEq} title="Import an AutoEQ ParametricEQ.txt">
            <FileInput size={15} /> Import AutoEQ
          </SecondaryButton>
          <SecondaryButton onClick={() => update({ ...flatEq(eq.enabled), autoHeadroom: eq.autoHeadroom })} title="Reset all bands to 0 dB">
            <RotateCcw size={15} /> Reset
          </SecondaryButton>
          {isSaved && (
            <SecondaryButton onClick={deletePreset} title="Delete this preset">
              <Trash2 size={15} />
            </SecondaryButton>
          )}
        </div>

        <div className="rounded-xl bg-panel p-4 ring-1 ring-line">
          <ResponseGraph eq={eq} onBand={setBand} />
        </div>

        <div className="flex flex-wrap items-center gap-x-8 gap-y-3 rounded-xl bg-panel px-5 py-4 ring-1 ring-line">
          <label className="flex items-center gap-3 text-[13px]">
            <span className="font-medium">Preamp</span>
            <input
              type="range"
              min={-24}
              max={12}
              step={0.1}
              value={eq.preamp}
              onChange={(e) => update({ ...eq, preamp: parseFloat(e.target.value) })}
              onDoubleClick={() => update({ ...eq, preamp: 0 })}
              className="slider w-56"
              style={{ ["--fill" as string]: `${((eq.preamp + 24) / 36) * 100}%` }}
              aria-label="Preamp"
            />
            <NumField value={+eq.preamp.toFixed(1)} onChange={(preamp) => update({ ...eq, preamp })} min={-30} max={30} suffix="dB" width="w-20" />
          </label>
          <label className="flex items-center gap-3 text-[13px]">
            <Toggle label="Automatic headroom" checked={eq.autoHeadroom} onChange={(v) => update({ ...eq, autoHeadroom: v })} />
            Auto headroom (prevent clipping)
          </label>
          <div className="ml-auto text-[12.5px] text-muted">
            Peak boost <span className="tabular text-fg">{fmtDb(Math.max(0, peak))} dB</span> · effective preamp{" "}
            <span className={`tabular font-medium ${effPre < 0 ? "text-accent" : "text-fg"}`}>{fmtDb(effPre)} dB</span>
          </div>
        </div>

        <div className="grid grid-cols-5 gap-2 lg:grid-cols-10">
          {eq.bands.map((b, i) => (
            <BandStrip key={i} band={b} index={i} onChange={(p) => setBand(i, p)} />
          ))}
        </div>
        <p className="pb-4 text-[11.5px] text-faint">
          Tip: drag the numbered points on the graph to move a band, scroll over a point to change its Q, double-click a
          slider to reset it. AutoEQ files ("ParametricEQ.txt") come from the AutoEQ project's per-headphone results.
        </p>
      </div>
    </div>
  );
}
