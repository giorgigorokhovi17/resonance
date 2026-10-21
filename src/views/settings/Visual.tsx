import { AlertTriangle, Check, Loader2, Play, RotateCcw } from "lucide-react";
import { useMemo, useState, type ReactNode } from "react";
import { api } from "../../lib/api";
import {
  ACCENT_MIN_CONTRAST,
  ACCENT_SWATCHES,
  BUTTON_MIN_CONTRAST,
  DEFAULT_APPEARANCE,
  PRESETS,
  presetById,
  resolveTheme,
  TEXT_MIN_CONTRAST,
} from "../../lib/theme";
import { importBackground, importError, pickMedia } from "../../lib/mediaImport";
import type { AnimatedArt, Appearance, Backdrop as BackdropOptions, Settings } from "../../lib/types";
import { Backdrop, useBackdropReadability } from "../../components/Backdrop";
import { useMedia } from "../../store/media";
import { usePlayer } from "../../store/player";
import { confirmDialog } from "../../components/Dialog";
import { SecondaryButton } from "../../components/EmptyState";
import { Slider } from "../../components/Slider";
import { useIsDark } from "../../hooks/useAppearance";
import { useLiveCover } from "../../store/library";
import { useCoverColor } from "../../hooks/useCoverColor";
import { Section, Segmented, Toggle } from "../SettingsView";

// Settings writes are applied locally first so sliders track the pointer; only the reply to the
// latest write is taken back from the backend.
let writeSeq = 0;
async function save(patch: Partial<Pick<Settings, "theme" | "appearance">>) {
  const st = usePlayer.getState();
  if (!st.settings) return;
  st.setSettings({ ...st.settings, ...patch });
  const seq = ++writeSeq;
  try {
    const s = await api.updateSettings(patch);
    if (seq === writeSeq) usePlayer.getState().setSettings(s);
  } catch (e) {
    usePlayer.getState().pushToast("error", `Could not save settings: ${e}`);
  }
}

function setAppearance(p: Partial<Appearance>) {
  const cur = usePlayer.getState().settings?.appearance ?? DEFAULT_APPEARANCE;
  save({ appearance: { ...cur, ...p } });
}

export function VisualSettings({ settings }: { settings: Settings }) {
  const a = settings.appearance ?? DEFAULT_APPEARANCE;
  const dark = useIsDark();
  const track = usePlayer((s) => s.status?.track ?? s.queue.items.find((i) => i.uid === s.queue.currentUid)?.track ?? null);
  const coverColor = useCoverColor(useLiveCover(track));
  const resolved = useMemo(() => resolveTheme(a, dark, coverColor), [a, dark, coverColor]);
  const customAccent = !!a.accent && !a.coverAccent;

  const reset = async () => {
    if (!(await confirmDialog("Reset visual settings?", "Theme, colours and layout go back to the defaults.", "Reset"))) return;
    save({ theme: "dark", appearance: DEFAULT_APPEARANCE });
  };

  return (
    <div className="space-y-5">
      <Section title="Theme" description="Each theme has a dark and a light version. The previews show the current mode.">
        <div className="space-y-5">
          <Row label="Mode">
            <Segmented
              value={settings.theme}
              onChange={(v) => save({ theme: v })}
              options={[
                { value: "dark", label: "Dark" },
                { value: "light", label: "Light" },
                { value: "system", label: "System", hint: "Follow the Windows app mode" },
              ]}
            />
          </Row>

          <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
            {PRESETS.map((p) => (
              <PresetCard
                key={p.id}
                id={p.id}
                name={p.name}
                appearance={a}
                dark={dark}
                selected={a.preset === p.id}
                onClick={() => setAppearance({ preset: p.id, accent: null, accent2: null })}
              />
            ))}
          </div>

          <Row label="Pure black background" hint="True black in dark mode. Suits OLED screens.">
            <Toggle label="Pure black background" checked={a.pureBlack} onChange={(v) => setAppearance({ pureBlack: v })} />
          </Row>
        </div>
      </Section>

      <Section title="Accent colour" description="Used for buttons, progress bars, and highlighted items.">
        <div className="space-y-5">
          <Row label="Match album art" hint="The accent follows the cover of the playing track.">
            <Toggle label="Match album art" checked={a.coverAccent} onChange={(v) => setAppearance({ coverAccent: v })} />
          </Row>

          <div className={a.coverAccent ? "pointer-events-none opacity-40" : ""}>
            <div className="mb-2 text-[12.5px] font-medium text-muted">Colours</div>
            <div className="flex flex-wrap items-center gap-2.5">
              <Swatch
                from={presetById(a.preset)[dark ? "dark" : "light"].accent}
                to={presetById(a.preset)[dark ? "dark" : "light"].accent2}
                selected={!a.accent}
                title={`${presetById(a.preset).name} (theme colours)`}
                onClick={() => setAppearance({ accent: null, accent2: null })}
              />
              <span className="mx-1 h-6 w-px bg-line" />
              {ACCENT_SWATCHES.map(([from, to]) => (
                <Swatch
                  key={from}
                  from={from}
                  to={to}
                  selected={a.accent === from && a.accent2 === to}
                  onClick={() => setAppearance({ accent: from, accent2: to })}
                />
              ))}
              <span className="mx-1 h-6 w-px bg-line" />
              <ColorInput
                label="Custom start colour"
                value={a.accent ?? resolved.vars["--c-accent"]}
                onChange={(c) => setAppearance({ accent: c, accent2: a.accent2 ?? resolved.vars["--c-accent2"] })}
              />
              <ColorInput
                label="Custom end colour"
                value={a.accent2 ?? resolved.vars["--c-accent2"]}
                onChange={(c) => setAppearance({ accent: a.accent ?? resolved.vars["--c-accent"], accent2: c })}
              />
              {customAccent && <span className="text-[11.5px] text-faint">Custom</span>}
            </div>
          </div>

          <ContrastCheck
            accent={resolved.accentContrast}
            button={resolved.buttonContrast}
            adjusted={resolved.adjusted}
            mode={dark ? "dark" : "light"}
          />
        </div>
      </Section>

      <Section title="Album art" description="Colour taken from the artwork tints the library and the Now Playing screen.">
        <div className="space-y-4">
          <Row label="Colour wash behind library views">
            <Toggle label="Colour wash" checked={a.ambient} onChange={(v) => setAppearance({ ambient: v })} />
          </Row>
          <Row label="Artwork intensity">
            <Slider
              value={a.ambientIntensity}
              min={0}
              max={1}
              step={0.05}
              format={(v) => `${Math.round(v * 100)}%`}
              onChange={(v) => setAppearance({ ambientIntensity: v })}
            />
          </Row>
          <Row label="Now Playing background blur">
            <Slider
              value={a.backdropBlur}
              min={0}
              max={160}
              step={5}
              format={(v) => `${Math.round(v)} px`}
              onChange={(v) => setAppearance({ backdropBlur: v })}
            />
          </Row>
        </div>
      </Section>

      <Section
        title="GIFs and videos"
        description="Let GIF and video covers and backgrounds move. Turning this on may use more resources (CPU, GPU and battery) while they play. When it's off, they show a still frame."
      >
        <div className="space-y-4">
          <Row label="Play GIFs and videos" hint="Off by default.">
            <Toggle
              label="Play GIFs and videos"
              checked={a.animatedArt !== "off"}
              onChange={(v) => setAppearance({ animatedArt: v ? "everywhere" : "off" })}
            />
          </Row>
          {a.animatedArt !== "off" && (
            <Row
              label="Where"
              hint="Grid tiles move while the pointer is over them. Nothing moves while Undertone is minimised or in the tray, or when Windows is set to show fewer animations."
            >
              <Segmented<AnimatedArt>
                value={a.animatedArt}
                onChange={(v) => setAppearance({ animatedArt: v })}
                options={[
                  { value: "everywhere", label: "Everywhere" },
                  { value: "nowPlaying", label: "Now Playing only" },
                ]}
              />
            </Row>
          )}
        </div>
      </Section>

      <Section
        title="Background"
        description="Your own photo or video behind the whole app, or only behind Now Playing. Videos loop without sound."
      >
        <BackgroundSettings a={a} dark={dark} />
      </Section>

      <Section title="Layout">
        <div className="space-y-4">
          <Row label="Corners">
            <Segmented<Appearance["corners"]>
              value={a.corners}
              onChange={(v) => setAppearance({ corners: v })}
              options={[
                { value: "sharp", label: "Sharp" },
                { value: "default", label: "Default" },
                { value: "round", label: "Round" },
              ]}
            />
          </Row>
          <Row label="Interface size" hint="Scales text and controls. The mini player always stays at 100%.">
            <Segmented
              value={String(a.uiScale)}
              onChange={(v) => setAppearance({ uiScale: parseFloat(v) })}
              options={["0.9", "1", "1.1", "1.25"].map((v) => ({ value: v, label: `${Math.round(parseFloat(v) * 100)}%` }))}
            />
          </Row>
        </div>
      </Section>

      <div className="flex justify-end">
        <SecondaryButton onClick={reset}>
          <RotateCcw size={14} /> Reset to defaults
        </SecondaryButton>
      </div>
    </div>
  );
}

function Row({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4">
      <span className="min-w-0">
        <span className="block text-[13px]">{label}</span>
        {hint && <span className="block text-[11.5px] text-faint">{hint}</span>}
      </span>
      {children}
    </div>
  );
}

function PresetCard({
  id,
  name,
  appearance,
  dark,
  selected,
  onClick,
}: {
  id: string;
  name: string;
  appearance: Appearance;
  dark: boolean;
  selected: boolean;
  onClick: () => void;
}) {
  const v = useMemo(
    () => resolveTheme({ ...appearance, preset: id, accent: null, accent2: null, coverAccent: false }, dark, null).vars,
    [appearance, id, dark],
  );
  const grad = `linear-gradient(135deg, ${v["--c-accent"]}, ${v["--c-accent2"]})`;
  return (
    <button
      onClick={onClick}
      aria-pressed={selected}
      className={`rounded-xl p-1.5 text-left transition-colors ${selected ? "bg-accent/10 ring-2 ring-accent" : "ring-1 ring-line hover:bg-hover"}`}
    >
      <div className="flex h-[78px] gap-1.5 overflow-hidden rounded-lg p-1.5" style={{ background: v["--c-bg"] }}>
        <div className="flex w-[26%] flex-col gap-1.5 pt-1">
          <div className="h-1.5 w-4/5 rounded-full" style={{ background: v["--c-muted"], opacity: 0.6 }} />
          <div className="h-1.5 w-3/5 rounded-full" style={{ background: v["--c-accent"] }} />
          <div className="h-1.5 w-2/3 rounded-full" style={{ background: v["--c-muted"], opacity: 0.6 }} />
        </div>
        <div className="flex flex-1 flex-col rounded-md p-2" style={{ background: v["--c-elev"] }}>
          <div className="h-2 w-3/4 rounded-full" style={{ background: v["--c-fg"] }} />
          <div className="mt-1.5 h-1.5 w-1/2 rounded-full" style={{ background: v["--c-muted"] }} />
          <div className="mt-auto flex items-center gap-1.5">
            <div className="h-3.5 w-3.5 rounded-full" style={{ background: grad }} />
            <div className="h-1 flex-1 rounded-full" style={{ background: v["--c-line"] }}>
              <div className="h-full w-1/2 rounded-full" style={{ background: grad }} />
            </div>
          </div>
        </div>
      </div>
      <div className="flex items-center justify-between px-1 pb-0.5 pt-1.5 text-[12.5px] font-medium">
        {name}
        {selected && <Check size={14} className="text-accent" />}
      </div>
    </button>
  );
}

function Swatch({
  from,
  to,
  selected,
  title,
  onClick,
}: {
  from: string;
  to: string;
  selected: boolean;
  title?: string;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      title={title ?? `${from} → ${to}`}
      aria-pressed={selected}
      className={`h-7 w-7 rounded-full transition-transform hover:scale-110 ${selected ? "ring-2 ring-fg ring-offset-2 ring-offset-panel" : ""}`}
      style={{ background: `linear-gradient(135deg, ${from}, ${to})` }}
    />
  );
}

function ColorInput({ label, value, onChange }: { label: string; value: string; onChange: (c: string) => void }) {
  return (
    <label
      title={label}
      className="relative h-7 w-7 cursor-pointer overflow-hidden rounded-full ring-1 ring-line transition-transform hover:scale-110"
      style={{ background: value }}
    >
      <input
        type="color"
        aria-label={label}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="absolute inset-0 h-full w-full cursor-pointer opacity-0"
      />
    </label>
  );
}

function ContrastCheck({ accent, button, adjusted, mode }: { accent: number; button: number; adjusted: boolean; mode: string }) {
  const ok = (r: number, min: number) => (
    <span className={`tabular inline-flex items-center gap-1 ${r >= min ? "text-ok" : "text-warn"}`}>
      {r >= min ? <Check size={13} /> : <AlertTriangle size={13} />}
      {r.toFixed(1)}:1
    </span>
  );
  return (
    <div className="flex flex-wrap items-center gap-x-6 gap-y-3 rounded-lg bg-bg p-4 ring-1 ring-line">
      <div className="flex items-center gap-3">
        <span className="accent-gradient inline-flex items-center gap-1.5 rounded-full px-4 py-1.5 text-[12.5px] font-semibold text-on-accent">
          <Play size={13} fill="currentColor" /> Play
        </span>
        <span className="text-[13px] font-semibold text-accent">Now playing</span>
      </div>
      <div className="space-y-0.5 text-[12px] text-muted">
        <div>
          Accent on background {ok(accent, ACCENT_MIN_CONTRAST)}
        </div>
        <div>
          Button text {ok(button, BUTTON_MIN_CONTRAST)}
        </div>
      </div>
      {adjusted && (
        <div className="basis-full text-[11.5px] text-faint">
          Adjusted slightly in {mode} mode so it stays readable.
        </div>
      )}
    </div>
  );
}

/** Settings → Visual → Background: none, a custom photo or a custom video, and how it shows. */
function BackgroundSettings({ a, dark }: { a: Appearance; dark: boolean }) {
  const bg = a.background;
  const media = useMedia((s) => (bg ? s.byId.get(bg.mediaId) : undefined));
  const [importing, setImporting] = useState(false);
  const read = useBackdropReadability(bg, "bg");
  const kind = !bg || !media ? "none" : media.kind === "video" ? "video" : "photo";

  const choose = async (k: "photo" | "video") => {
    const picked = await pickMedia(k === "video" ? "Choose a background video" : "Choose a background photo", k);
    if (!picked) return;
    setImporting(true);
    try {
      const m = await importBackground(picked);
      const cur = usePlayer.getState().settings?.appearance.background;
      setAppearance({ background: { fit: "cover", dim: null, blur: 0, ...cur, mediaId: m.id } });
    } catch (e) {
      usePlayer.getState().pushToast("error", importError(e));
    } finally {
      setImporting(false);
    }
  };
  const set = (p: Partial<BackdropOptions>) => bg && setAppearance({ background: { ...bg, ...p } });
  const remove = async () => {
    if (!(await confirmDialog("Remove the custom background?", "The app goes back to the theme's colours.", "Remove"))) return;
    setAppearance({ background: null, backgroundScope: "app" });
  };
  const reset = () => bg && setAppearance({ background: { ...bg, fit: "cover", dim: null, blur: 0 }, backgroundScope: "app" });

  return (
    <div className="space-y-4">
      <Row label="Background" hint="Photos: JPG, PNG, WebP or GIF. Videos: MP4 (H.264) or WebM, up to 60 s and 1080p.">
        <div className="flex shrink-0 items-center gap-2">
          {importing && <Loader2 size={15} className="animate-spin text-muted" />}
          <Segmented
            value={kind}
            onChange={(v) => (v === "none" ? (bg ? remove() : undefined) : choose(v))}
            options={[
              { value: "none", label: "None" },
              { value: "photo", label: "Custom photo" },
              { value: "video", label: "Custom video" },
            ]}
          />
        </div>
      </Row>
      {bg && media && media.kind !== "image" && a.animatedArt === "off" && (
        <p className="text-[12px] text-faint">GIFs and videos are turned off (above), so this shows as a still frame.</p>
      )}
      {bg && media && (
        <>
          <div className="flex flex-wrap items-start gap-5">
            <SkinPreview backdrop={bg} />
            <div className="min-w-[240px] flex-1 space-y-3">
              <Row label="Show it on">
                <Segmented
                  value={a.backgroundScope}
                  onChange={(v) => setAppearance({ backgroundScope: v })}
                  options={[
                    { value: "app", label: "Whole app" },
                    { value: "nowPlaying", label: "Now Playing" },
                  ]}
                />
              </Row>
              <Row label="Fit">
                <Segmented
                  value={bg.fit}
                  onChange={(v) => set({ fit: v })}
                  options={[
                    { value: "cover", label: "Fill" },
                    { value: "contain", label: "Whole picture" },
                  ]}
                />
              </Row>
              <Row label="Dim" hint="Raised by itself if text would be hard to read.">
                <Slider label="Dim" width="w-36" value={read.chosen} min={0} max={0.8} step={0.01} format={(v) => `${Math.round(v * 100)}%`} onChange={(v) => set({ dim: v })} />
              </Row>
              <Row label="Blur">
                <Slider label="Blur" width="w-36" value={bg.blur} min={0} max={40} step={1} format={(v) => `${v} px`} onChange={(v) => set({ blur: v })} />
              </Row>
            </div>
          </div>
          <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg bg-bg p-3 ring-1 ring-line">
            <div className="text-[12px] text-muted">
              Text over the background{" "}
              {read.ready ? (
                <span className={`tabular inline-flex items-center gap-1 ${read.contrast >= TEXT_MIN_CONTRAST ? "text-ok" : "text-warn"}`}>
                  {read.contrast >= TEXT_MIN_CONTRAST ? <Check size={13} /> : <AlertTriangle size={13} />}
                  {read.contrast.toFixed(1)}:1
                </span>
              ) : (
                "…"
              )}
              {read.raised && (
                <span className="block text-[11.5px] text-faint">
                  Dimmed to {Math.round(read.dim * 100)}% in {dark ? "dark" : "light"} mode so text stays readable.
                </span>
              )}
            </div>
            <div className="flex gap-2">
              <SecondaryButton onClick={() => choose(kind === "video" ? "video" : "photo")}>Change…</SecondaryButton>
              <SecondaryButton onClick={reset} title="Fill, default dim, no blur, whole app">
                <RotateCcw size={14} /> Reset
              </SecondaryButton>
            </div>
          </div>
        </>
      )}
    </div>
  );
}

/** A small mock of the app over the chosen background, with the same scrim. */
function SkinPreview({ backdrop }: { backdrop: BackdropOptions }) {
  const line = (w: string, cls = "bg-fg/80") => <div className={`h-1.5 rounded-full ${cls}`} style={{ width: w }} />;
  return (
    <div className="relative isolate aspect-video w-[300px] shrink-0 overflow-hidden rounded-lg bg-bg ring-1 ring-line">
      <Backdrop backdrop={backdrop} place="page" scrim="bg" className="absolute inset-0 -z-10" />
      <div className="flex h-full flex-col gap-1.5 p-1.5">
        <div className="flex min-h-0 flex-1 gap-1.5">
          <div className="w-[26%] space-y-2 rounded-md p-2" style={{ background: "color-mix(in srgb, var(--c-elev) 40%, transparent)" }}>
            {line("70%")}
            {line("55%", "bg-accent")}
            {line("62%", "bg-muted")}
          </div>
          <div className="flex-1 rounded-md p-2.5" style={{ background: "color-mix(in srgb, var(--c-elev) 40%, transparent)" }}>
            <div className="text-[13px] font-bold leading-tight">Songs</div>
            <div className="text-[9.5px] text-muted">12 songs · 45 min</div>
            <div className="mt-2 space-y-1">
              {["Track one", "Track two", "Track three"].map((t) => (
                <div key={t} className="truncate text-[9.5px]">
                  {t} <span className="text-muted">· Artist</span>
                </div>
              ))}
            </div>
          </div>
        </div>
        <div className="flex h-5 items-center gap-1.5 px-1">
          <div className="h-3.5 w-3.5 rounded-full bg-fg" />
          <div className="h-1 flex-1 rounded-full bg-fg/20">
            <div className="accent-gradient h-full w-1/3 rounded-full" />
          </div>
        </div>
      </div>
    </div>
  );
}
