import { Check, RefreshCw, Speaker } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { api } from "../lib/api";
import type { DeviceInfo, OutputMode, Settings } from "../lib/types";
import { usePlayer } from "../store/player";
import { FormatBadge } from "../components/FormatBadge";
import { BackupSection } from "./settings/Backup";
import { LibraryFoldersSection } from "./settings/LibraryFolders";
import { UpdatesSection } from "./settings/Updates";
import { VisualSettings } from "./settings/Visual";

type Tab = "general" | "visual";
// Remembered while the app runs, so leaving Settings and coming back keeps the tab.
let lastTab: Tab = "general";

export function Section({ title, description, children }: { title: string; description?: string; children: ReactNode }) {
  return (
    <section className="rounded-xl bg-panel p-5 ring-1 ring-line">
      <h2 className="text-[15px] font-semibold">{title}</h2>
      {description && <p className="mt-1 text-[12.5px] leading-relaxed text-muted">{description}</p>}
      <div className="mt-4">{children}</div>
    </section>
  );
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
}: {
  value: T;
  options: { value: T; label: string; hint?: string }[];
  onChange: (v: T) => void;
}) {
  return (
    <div className="inline-flex rounded-lg bg-bg p-1 ring-1 ring-line">
      {options.map((o) => (
        <button
          key={o.value}
          title={o.hint}
          onClick={() => onChange(o.value)}
          className={`rounded-md px-3.5 py-1.5 text-[12.5px] font-medium transition-colors ${
            value === o.value ? "bg-panel text-fg shadow ring-1 ring-line" : "text-muted hover:text-fg"
          }`}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Toggle({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => onChange(!checked)}
      className={`relative h-5 w-9 shrink-0 rounded-full transition-colors ${checked ? "bg-accent" : "bg-fg/20"}`}
    >
      <span
        className={`absolute left-0 top-0.5 h-4 w-4 rounded-full bg-white shadow transition-transform ${checked ? "translate-x-[18px]" : "translate-x-0.5"}`}
      />
    </button>
  );
}

export function SettingsView() {
  const settings = usePlayer((s) => s.settings);
  const setSettings = usePlayer((s) => s.setSettings);
  const hasSource = usePlayer((s) => !!s.status?.source);
  const [devices, setDevices] = useState<DeviceInfo[]>([]);
  const [loadingDevices, setLoadingDevices] = useState(false);
  const [tab, setTabState] = useState<Tab>(lastTab);
  const setTab = (t: Tab) => {
    lastTab = t;
    setTabState(t);
  };

  const refreshDevices = async () => {
    setLoadingDevices(true);
    try {
      setDevices(await api.listDevices());
    } catch (e) {
      usePlayer.getState().pushToast("error", `Could not list audio devices: ${e}`);
    } finally {
      setLoadingDevices(false);
    }
  };

  useEffect(() => {
    refreshDevices();
  }, []);

  if (!settings) return null;

  const setOutput = async (mode: OutputMode, deviceId: string | null) => {
    setSettings(await api.setOutput(mode, deviceId));
  };
  const patch = async (p: Parameters<typeof api.updateSettings>[0]) => {
    setSettings(await api.updateSettings(p));
  };

  return (
    <div className="mx-auto max-w-3xl space-y-5 px-8 py-6 animate-in">
      <div className="flex flex-wrap items-center justify-between gap-4">
        <h1 className="font-display text-[28px] font-semibold">Settings</h1>
        <Segmented<Tab>
          value={tab}
          onChange={setTab}
          options={[
            { value: "general", label: "General" },
            { value: "visual", label: "Visual" },
          ]}
        />
      </div>

      {tab === "visual" ? (
        <VisualSettings settings={settings} />
      ) : (
        <>
          <Section
            title="Audio output"
            description="Shared mode plays through the Windows mixer, so system-wide effects such as Equalizer APO still apply. Exclusive mode takes over the device and opens it at each file's native sample rate and bit depth: no mixer, no resampling. With volume at 100% and the EQ off, the signal is bit-perfect."
          >
            <div className="space-y-5">
              <Segmented<OutputMode>
                value={settings.outputMode}
                onChange={(m) => setOutput(m, settings.deviceId)}
                options={[
                  { value: "shared", label: "Shared (WASAPI)" },
                  { value: "exclusive", label: "Exclusive (bit-perfect)" },
                ]}
              />

              <div>
                <div className="mb-2 flex items-center justify-between">
                  <span className="text-[12.5px] font-medium text-muted">Output device</span>
                  <button
                    onClick={refreshDevices}
                    className="flex items-center gap-1.5 rounded-md px-2 py-1 text-[12px] text-muted hover:bg-hover hover:text-fg"
                  >
                    <RefreshCw size={13} className={loadingDevices ? "animate-spin" : ""} /> Refresh
                  </button>
                </div>
                <div className="overflow-hidden rounded-lg ring-1 ring-line">
                  <DeviceRow
                    name="System default"
                    hint="Follows the Windows default output device"
                    selected={!settings.deviceId}
                    onClick={() => setOutput(settings.outputMode, null)}
                  />
                  {devices.map((d) => (
                    <DeviceRow
                      key={d.id}
                      name={d.name}
                      hint={d.isDefault ? "Current Windows default" : undefined}
                      selected={settings.deviceId === d.id}
                      onClick={() => setOutput(settings.outputMode, d.id)}
                    />
                  ))}
                </div>
              </div>

              <div className="rounded-lg bg-bg p-4 ring-1 ring-line">
                <div className="mb-2 text-[12px] font-medium uppercase tracking-wider text-faint">Signal path</div>
                {hasSource ? (
                  <FormatBadge detailed />
                ) : (
                  <div className="text-[12.5px] text-muted">Start playback to see the live format.</div>
                )}
              </div>
            </div>
          </Section>

          <Section
            title="ReplayGain"
            description="Normalizes loudness using ReplayGain tags. Gain is limited by the stored peak so it never clips. Turning it on means the output is no longer bit-perfect."
          >
            <div className="flex flex-wrap items-center gap-6">
              <Segmented
                value={settings.replayGain}
                onChange={(v) => patch({ replayGain: v })}
                options={[
                  { value: "off", label: "Off" },
                  { value: "track", label: "Track" },
                  { value: "album", label: "Album" },
                ]}
              />
              <label className="flex items-center gap-3 text-[12.5px] text-muted">
                Pre-amp
                <input
                  type="range"
                  min={-15}
                  max={15}
                  step={0.5}
                  value={settings.replayGainPreamp}
                  className="slider w-40"
                  style={{ ["--fill" as string]: `${((settings.replayGainPreamp + 15) / 30) * 100}%` }}
                  onChange={(e) => patch({ replayGainPreamp: parseFloat(e.target.value) })}
                />
                <span className="tabular w-14 text-fg">
                  {settings.replayGainPreamp > 0 ? "+" : ""}
                  {settings.replayGainPreamp.toFixed(1)} dB
                </span>
              </label>
            </div>
          </Section>

          <LibraryFoldersSection />

          <BackupSection />

          <UpdatesSection />

          <Section title="Behavior" description="Theme, colours and layout are on the Visual tab.">
            <div className="space-y-4">
              <Row label="Close button minimizes to tray">
                <Toggle label="Close to tray" checked={settings.closeToTray} onChange={(v) => patch({ closeToTray: v })} />
              </Row>
              <Row label="Watch library folders for changes">
                <Toggle label="Watch folders" checked={settings.watchFolders} onChange={(v) => patch({ watchFolders: v })} />
              </Row>
            </div>
          </Section>

          <Section title="Keyboard shortcuts">
            <ShortcutList />
          </Section>
        </>
      )}

      <p className="pb-6 text-center text-[11.5px] text-faint">
        Resonance runs locally. No accounts, no telemetry; the only network request is the update check.
      </p>
    </div>
  );
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4">
      <span className="text-[13px]">{label}</span>
      {children}
    </div>
  );
}

function DeviceRow({ name, hint, selected, onClick }: { name: string; hint?: string; selected: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className={`flex w-full items-center gap-3 border-b border-line px-4 py-2.5 text-left last:border-b-0 ${
        selected ? "bg-accent/10" : "hover:bg-hover"
      }`}
    >
      <Speaker size={16} className={selected ? "text-accent" : "text-faint"} />
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[13px]">{name}</span>
        {hint && <span className="block text-[11.5px] text-faint">{hint}</span>}
      </span>
      {selected && <Check size={16} className="text-accent" />}
    </button>
  );
}

const SHORTCUTS: [string, string][] = [
  ["Space", "Play / pause"],
  ["Ctrl + → / ←", "Next / previous track"],
  ["→ / ←", "Seek ±5 s (Shift: ±30 s)"],
  ["↑ / ↓", "Volume up / down"],
  ["M", "Mute"],
  ["S / R", "Shuffle / repeat"],
  ["F", "Now playing"],
  ["Q", "Queue panel"],
  ["Ctrl + F", "Search"],
  ["Ctrl + M", "Mini player"],
  ["Alt + ← / →", "Back / forward"],
];

function ShortcutList() {
  return (
    <div className="grid grid-cols-1 gap-x-8 gap-y-2 sm:grid-cols-2">
      {SHORTCUTS.map(([k, v]) => (
        <div key={k} className="flex items-center justify-between gap-3 text-[12.5px]">
          <span className="text-muted">{v}</span>
          <kbd className="rounded bg-bg px-1.5 py-0.5 font-sans text-[11px] text-fg ring-1 ring-line">{k}</kbd>
        </div>
      ))}
    </div>
  );
}

export type { Settings };
