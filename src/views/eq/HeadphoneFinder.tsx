import { Check, Headphones, Loader2, Search, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { curve } from "../../lib/eqmath";
import type { EqBand, EqSettings, FilterType } from "../../lib/types";

/**
 * Headphone EQ finder: searches the bundled AutoEQ database (public/autoeq.json, built by
 * scripts/build-autoeq.mjs) and recommends a correction profile plus settings for the model.
 */

type Kind = "over-ear" | "in-ear" | "earbud";
/** [name, source, rig, kind, preamp, [type, fc, gain, q, ...]] */
type RawEntry = [string, string, string, Kind, number, (string | number)[]];

interface Profile {
  name: string;
  source: string;
  rig: string;
  kind: Kind;
  preamp: number;
  bands: EqBand[];
}

interface Model {
  name: string;
  key: string;
  kind: Kind;
  /** Best measurement first. */
  profiles: Profile[];
}

const TYPE: Record<string, FilterType> = { PK: "peaking", LSC: "lowShelf", HSC: "highShelf", LP: "lowPass", HP: "highPass" };

// Most trusted measurement sources first (roughly AutoEQ's own ranking).
const SOURCE_RANK = [
  "oratory1990",
  "crinacle",
  "Rtings",
  "Innerfidelity",
  "Headphone.com Legacy",
  "HypetheSonics",
  "Kuulokenurkka",
  "Super Review",
];

const norm = (s: string) => s.toLowerCase().normalize("NFKD").replace(/[^a-z0-9]+/g, " ").trim();

function rank(p: Profile): number {
  const i = SOURCE_RANK.indexOf(p.source);
  // crinacle's GRAS measurements are more reliable than his 711 coupler ones for over-ears.
  return (i < 0 ? SOURCE_RANK.length : i) * 2 + (p.source === "crinacle" && !/gras/i.test(p.rig) ? 1 : 0);
}

let cache: Promise<Model[]> | null = null;
function loadModels(): Promise<Model[]> {
  cache ??= fetch("/autoeq.json")
    .then((r) => {
      if (!r.ok) throw new Error(`Headphone database missing (HTTP ${r.status})`);
      return r.json() as Promise<{ entries: RawEntry[] }>;
    })
    .then(({ entries }) => {
      const byName = new Map<string, Model>();
      for (const [name, source, rig, kind, preamp, f] of entries) {
        const bands: EqBand[] = [];
        for (let i = 0; i + 3 < f.length; i += 4) {
          const filter = TYPE[f[i] as string];
          if (filter) bands.push({ enabled: true, filter, freq: +f[i + 1], gain: +f[i + 2], q: +f[i + 3] });
        }
        // Merge spellings that only differ in case/spacing ("AirPods Pro2" / "Airpods Pro 2").
        const id = norm(name).replace(/ /g, "");
        let m = byName.get(id);
        if (!m) byName.set(id, (m = { name, key: norm(name), kind, profiles: [] }));
        m.profiles.push({ name, source, rig, kind, preamp, bands });
      }
      const models = [...byName.values()];
      for (const m of models) m.profiles.sort((a, b) => rank(a) - rank(b));
      return models;
    })
    .catch((e) => {
      cache = null;
      throw e;
    });
  return cache;
}

function search(models: Model[], q: string): Model[] {
  const tokens = norm(q).split(" ").filter(Boolean);
  if (!tokens.length) return [];
  const hits = models.filter((m) => tokens.every((t) => m.key.includes(t)));
  const first = tokens[0];
  hits.sort(
    (a, b) =>
      Number(!a.key.startsWith(first)) - Number(!b.key.startsWith(first)) ||
      a.key.length - b.key.length ||
      a.name.localeCompare(b.name),
  );
  return hits.slice(0, 60);
}

/** Taste adjustments applied on top of the neutral correction (to its bass and treble shelves). */
const TASTES = [
  { id: "neutral", label: "Neutral", hint: "Recommended: the plain correction", bass: 0, treble: 0 },
  { id: "warm", label: "Warm", hint: "A little more body, softer highs", bass: 2, treble: -1.5 },
  { id: "bass", label: "Bass boost", hint: "+4 dB below ~100 Hz", bass: 4, treble: 0 },
  { id: "bright", label: "Bright", hint: "+2 dB air above ~10 kHz", bass: 0, treble: 2 },
  { id: "relaxed", label: "Relaxed treble", hint: "Tames sibilance and harshness", bass: 0, treble: -2.5 },
] as const;
type TasteId = (typeof TASTES)[number]["id"];

const KIND_LABEL: Record<Kind, string> = { "over-ear": "Over-ear", "in-ear": "In-ear", earbud: "Earbud" };

const FIT_TIP: Record<Kind, string> = {
  "over-ear":
    "Seat the cups the same way each time. Worn pads or glasses arms break the seal and cost bass; add a little with Warm or Bass boost if needed.",
  "in-ear":
    "Bass depends on the seal. If it sounds thin after applying, try a larger or foam tip before adding bass. Insert depth also moves the treble peak.",
  earbud: "Earbuds don't seal, so bass varies a lot between ears. Bass boost is often a better match than Neutral.",
};

export function buildEq(p: Profile, tasteId: TasteId): EqSettings {
  const taste = TASTES.find((t) => t.id === tasteId)!;
  const bands = p.bands.map((b) => ({ ...b }));
  const ls = bands.find((b) => b.filter === "lowShelf");
  const hs = bands.find((b) => b.filter === "highShelf");
  if (ls) ls.gain = +(ls.gain + taste.bass).toFixed(1);
  if (hs) hs.gain = +(hs.gain + taste.treble).toFixed(1);
  // The engine always has 10 bands; pad shorter profiles with neutral, disabled ones.
  while (bands.length < 10) bands.push({ enabled: false, filter: "peaking", freq: 1000, gain: 0, q: 1 });
  const suffix = taste.id === "neutral" ? "" : ` · ${taste.label}`;
  return {
    enabled: true,
    // Extra boost from the taste needs matching headroom.
    preamp: +(p.preamp - Math.max(0, taste.bass, taste.treble)).toFixed(1),
    autoHeadroom: true,
    bands: bands.slice(0, 10),
    presetName: `${p.name} (AutoEQ${suffix})`,
  };
}

function MiniCurve({ eq }: { eq: EqSettings }) {
  const w = 520;
  const h = 120;
  const pts = useMemo(() => curve({ ...eq, preamp: 0, autoHeadroom: false }, 160), [eq]);
  const range = Math.max(6, Math.ceil(Math.max(...pts.map((p) => Math.abs(p.db))) / 3) * 3);
  const x = (f: number) => ((Math.log10(f) - Math.log10(20)) / 3) * w;
  const y = (db: number) => h / 2 - (db / range) * (h / 2 - 6);
  const d = pts.map((p, i) => `${i ? "L" : "M"}${x(p.f).toFixed(1)},${y(p.db).toFixed(1)}`).join("");
  return (
    <svg viewBox={`0 0 ${w} ${h}`} className="h-auto w-full" aria-label="Correction curve">
      {[100, 1000, 10000].map((f) => (
        <line key={f} x1={x(f)} x2={x(f)} y1={0} y2={h} stroke="var(--c-line)" vectorEffect="non-scaling-stroke" />
      ))}
      <line x1={0} x2={w} y1={h / 2} y2={h / 2} stroke="var(--c-muted)" strokeDasharray="4 3" vectorEffect="non-scaling-stroke" />
      <path d={d} fill="none" stroke="var(--c-accent)" strokeWidth={2} vectorEffect="non-scaling-stroke" />
      <text x={4} y={12} fontSize={10} fill="var(--c-faint)">
        +{range} dB
      </text>
      <text x={4} y={h - 4} fontSize={10} fill="var(--c-faint)">
        −{range} dB
      </text>
    </svg>
  );
}

function loadSaved(): { name: string; source: string; taste: TasteId } | null {
  try {
    return JSON.parse(localStorage.getItem("eq.headphone") ?? "null");
  } catch {
    return null;
  }
}

export function HeadphoneFinder({ onApply }: { onApply: (eq: EqSettings) => Promise<void> | void }) {
  const [models, setModels] = useState<Model[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState(false);
  const [model, setModel] = useState<Model | null>(null);
  const [profileIdx, setProfileIdx] = useState(0);
  const [taste, setTaste] = useState<TasteId>("neutral");
  const [applied, setApplied] = useState(false);
  const boxRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    loadModels()
      .then((ms) => {
        setModels(ms);
        // Re-open the headphones picked last time.
        const saved = loadSaved();
        const m = saved && ms.find((x) => x.name === saved.name);
        if (m) {
          setModel(m);
          setProfileIdx(Math.max(0, m.profiles.findIndex((p) => p.source === saved!.source)));
          setTaste(saved!.taste ?? "neutral");
        }
      })
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => !boxRef.current?.contains(e.target as Node) && setOpen(false);
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [open]);

  const results = useMemo(() => (models ? search(models, query) : []), [models, query]);
  const profile = model?.profiles[profileIdx] ?? null;
  const eq = useMemo(() => (profile ? buildEq(profile, taste) : null), [profile, taste]);
  useEffect(() => setApplied(false), [eq]);

  const pick = (m: Model) => {
    setModel(m);
    setProfileIdx(0);
    setTaste("neutral");
    setQuery("");
    setOpen(false);
  };

  const apply = async () => {
    if (!eq || !profile) return;
    await onApply(eq);
    setApplied(true);
    try {
      localStorage.setItem("eq.headphone", JSON.stringify({ name: model!.name, source: profile.source, taste }));
    } catch {
      /* ignore */
    }
  };

  return (
    <div className="rounded-xl bg-panel p-4 ring-1 ring-line">
      <div className="flex flex-wrap items-center gap-3">
        <div className="flex items-center gap-2">
          <Headphones size={18} className="text-accent" />
          <h2 className="text-[14px] font-semibold">Headphone EQ finder</h2>
        </div>
        <div ref={boxRef} className="relative min-w-[260px] flex-1">
          <label className="flex h-9 items-center gap-2 rounded-full bg-bg px-3.5 ring-1 ring-line focus-within:ring-accent">
            <Search size={15} className="shrink-0 text-faint" />
            <input
              value={query}
              onChange={(e) => {
                setQuery(e.target.value);
                setOpen(true);
              }}
              onFocus={() => setOpen(true)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && results[0]) pick(results[0]);
                if (e.key === "Escape") setOpen(false);
              }}
              placeholder={
                models ? `Search ${models.length.toLocaleString()} headphones and earphones, e.g. "HD 600" or "AirPods Pro"` : "Loading headphone database…"
              }
              disabled={!models}
              aria-label="Search headphone model"
              className="min-w-0 flex-1 bg-transparent text-[13px] outline-none placeholder:text-faint"
            />
            {!models && !error && <Loader2 size={15} className="animate-spin text-faint" />}
            {query && (
              <button aria-label="Clear search" onClick={() => setQuery("")} className="text-faint hover:text-fg">
                <X size={14} />
              </button>
            )}
          </label>
          {open && query.trim() && models && (
            <div className="absolute left-0 right-0 top-11 z-30 max-h-80 overflow-y-auto rounded-lg bg-panel py-1 shadow-2xl ring-1 ring-line">
              {results.length === 0 ? (
                <div className="px-3 py-3 text-[12.5px] text-muted">No match. Try just the model number, e.g. "M50x".</div>
              ) : (
                results.map((m) => (
                  <button
                    key={m.name}
                    onClick={() => pick(m)}
                    className="flex w-full items-center gap-3 px-3 py-1.5 text-left text-[13px] hover:bg-hover"
                  >
                    <span className="min-w-0 flex-1 truncate">{m.name}</span>
                    <span className="shrink-0 text-[11px] text-faint">
                      {KIND_LABEL[m.kind]}
                      {m.profiles.length > 1 ? ` · ${m.profiles.length} measurements` : ""}
                    </span>
                  </button>
                ))
              )}
            </div>
          )}
        </div>
      </div>
      {error && <p className="mt-3 text-[12.5px] text-[#ef5b5b]">{error}</p>}

      {model && profile && eq && (
        <div className="mt-4 grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.1fr)]">
          <div className="min-w-0 space-y-3">
            <div>
              <div className="text-[16px] font-semibold">{model.name}</div>
              <div className="mt-0.5 text-[12px] text-muted">
                {KIND_LABEL[model.kind]} · corrects it toward AutoEQ's default {model.kind === "over-ear" ? "over-ear" : "in-ear"} target
                (Harman-style neutral)
              </div>
            </div>
            {model.profiles.length > 1 && (
              <label className="flex items-center gap-2 text-[12.5px] text-muted">
                Measurement
                <select
                  value={profileIdx}
                  onChange={(e) => setProfileIdx(+e.target.value)}
                  className="min-w-0 flex-1 rounded-md bg-bg px-2 py-1 text-[12.5px] text-fg outline-none ring-1 ring-line focus:ring-accent"
                >
                  {model.profiles.map((p, i) => (
                    <option key={i} value={i}>
                      {p.source}
                      {p.rig ? ` (${p.rig})` : ""}
                      {i === 0 ? " · recommended" : ""}
                    </option>
                  ))}
                </select>
              </label>
            )}
            <div>
              <div className="mb-1.5 text-[12.5px] text-muted">Sound preference</div>
              <div className="flex flex-wrap gap-1.5">
                {TASTES.map((t) => (
                  <button
                    key={t.id}
                    onClick={() => setTaste(t.id)}
                    title={t.hint}
                    className={`rounded-full px-3 py-1 text-[12px] ring-1 transition-colors ${
                      taste === t.id ? "bg-accent/15 text-fg ring-accent" : "text-muted ring-line hover:bg-hover hover:text-fg"
                    }`}
                  >
                    {t.label}
                  </button>
                ))}
              </div>
            </div>
            <div className="rounded-lg bg-bg/60 p-3 ring-1 ring-line">
              <div className="mb-1.5 text-[12px] font-semibold uppercase tracking-wider text-muted">Recommended settings</div>
              <ul className="space-y-1 text-[12.5px] leading-relaxed">
                <li>
                  <span className="text-muted">EQ:</span> {eq.bands.filter((b) => b.enabled).length}-band correction measured by{" "}
                  {profile.source}
                  {profile.rig ? ` on ${profile.rig}` : ""}
                </li>
                <li>
                  <span className="text-muted">Preamp:</span> <span className="tabular">{eq.preamp.toFixed(1)} dB</span>, with auto
                  headroom on, so boosted bands can't clip. Turn the volume up a little to make up for it.
                </li>
                <li>
                  <span className="text-muted">Fit:</span> {FIT_TIP[model.kind]}
                </li>
                <li className="text-faint">
                  Units vary between production batches, so treat this as a starting point and adjust the bands below to taste.
                </li>
              </ul>
            </div>
          </div>
          <div className="flex min-w-0 flex-col gap-3">
            <div className="rounded-lg bg-bg/60 p-2 ring-1 ring-line">
              <MiniCurve eq={eq} />
              <div className="flex justify-between px-1 text-[10px] text-faint">
                <span>20 Hz</span>
                <span>100</span>
                <span>1k</span>
                <span>10k</span>
                <span>20k</span>
              </div>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <button
                onClick={apply}
                className="accent-gradient inline-flex items-center gap-2 rounded-full px-5 py-2 text-[13px] font-semibold text-white shadow-md transition-transform hover:scale-[1.03] active:scale-[0.98]"
              >
                {applied ? <Check size={16} /> : <Headphones size={16} />} {applied ? "Applied" : "Apply recommended EQ"}
              </button>
              <span className="text-[11.5px] text-faint">Also saved as a preset. Profiles: AutoEQ project (MIT).</span>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
