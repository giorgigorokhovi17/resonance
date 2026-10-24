import { Check, Move } from "lucide-react";
import type { ReactNode } from "react";
import { api } from "../../lib/api";
import { ISLAND_PALETTES, POSITION_GRID, POSITION_LABELS } from "../../lib/island";
import type { Palette } from "../../lib/palette";
import type { IslandOptions, Settings } from "../../lib/types";
import { usePlayer } from "../../store/player";
import { Section, Toggle } from "../SettingsView";

// Applied locally first so colour pickers track the pointer; only the reply to the latest write
// is taken back from the backend.
let writeSeq = 0;
async function save(patch: Partial<Pick<Settings, "island" | "islandOptions">>) {
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

function setOptions(p: Partial<IslandOptions>) {
  const cur = usePlayer.getState().settings?.islandOptions;
  if (cur) save({ islandOptions: { ...cur, ...p } });
}

/** Settings → General → Dynamic Island. Most of this is also on the island's right-click menu. */
export function IslandSection({ settings }: { settings: Settings }) {
  const o = settings.islandOptions;
  return (
    <Section
      title="Dynamic Island"
      description="A small bar with the playing song that stays on top of other apps. Point at it for controls, progress and the current lyric line, drag it anywhere, right-click it for options, and double-click it to open Undertone."
    >
      <div className="space-y-4">
        <Row label="Show the Dynamic Island">
          <Toggle label="Dynamic Island" checked={settings.island} onChange={(v) => save({ island: v })} />
        </Row>
        {settings.island && (
          <>
            <Row label="Only while Undertone is minimised" hint="Off: it stays up while you use Undertone too.">
              <Toggle label="Only while minimised" checked={o.onlyWhenMinimized} onChange={(v) => setOptions({ onlyWhenMinimized: v })} />
            </Row>

            <Row
              label="Hide over full-screen games and videos"
              hint="Off: it stays on top of them. Games in exclusive full screen hide everything; use their borderless mode."
            >
              <Toggle label="Hide over full screen" checked={o.hideInFullscreen} onChange={(v) => setOptions({ hideInFullscreen: v })} />
            </Row>

            <div className="flex flex-wrap items-start justify-between gap-4">
              <span className="min-w-0">
                <span className="block text-[13px]">Position</span>
                <span className="block text-[11.5px] text-faint">
                  {o.position === "custom" ? "Where you dragged it. Pick a spot to snap it back." : POSITION_LABELS[o.position]}
                </span>
              </span>
              <div className="grid grid-cols-3 gap-1 rounded-lg bg-bg p-1.5 ring-1 ring-line">
                {POSITION_GRID.map((p, i) =>
                  p ? (
                    <button
                      key={p}
                      title={POSITION_LABELS[p]}
                      aria-label={POSITION_LABELS[p]}
                      aria-pressed={o.position === p}
                      onClick={() => setOptions({ position: p })}
                      className={`flex h-7 w-10 items-center justify-center rounded transition-colors ${
                        o.position === p ? "bg-accent text-on-accent" : "bg-panel hover:bg-hover"
                      }`}
                    >
                      <span className={`h-1.5 w-4 rounded-full ${o.position === p ? "bg-on-accent" : "bg-fg/30"}`} />
                    </button>
                  ) : (
                    <span key={i} className="flex h-7 w-10 items-center justify-center text-faint" title="Drag the island to put it anywhere">
                      {o.position === "custom" && <Move size={13} className="text-accent" />}
                    </span>
                  ),
                )}
              </div>
            </div>

            <Row label="Lock position" hint="It can't be dragged.">
              <Toggle label="Lock position" checked={o.locked} onChange={(v) => setOptions({ locked: v })} />
            </Row>
            <Row label="Let clicks through" hint="Clicks go to the app underneath. Hold Alt to use the island anyway.">
              <Toggle label="Let clicks through" checked={o.clickThrough} onChange={(v) => setOptions({ clickThrough: v })} />
            </Row>
            <Row label="Open when pointed at" hint="Grows into bigger controls, progress and the current lyric line.">
              <Toggle label="Open when pointed at" checked={o.expandOnHover} onChange={(v) => setOptions({ expandOnHover: v })} />
            </Row>
            <Row label="Pop up when a song starts" hint="A small card with the new song for a few seconds.">
              <Toggle label="Pop up when a song starts" checked={o.peekOnNewSong} onChange={(v) => setOptions({ peekOnNewSong: v })} />
            </Row>
            <Row label="Show the song title">
              <Toggle label="Show the song title" checked={o.showTitle} onChange={(v) => setOptions({ showTitle: v })} />
            </Row>
            <Row label="Show the artist">
              <Toggle label="Show the artist" checked={o.showArtist} onChange={(v) => setOptions({ showArtist: v })} />
            </Row>
            <Row label="Show play controls" hint="Previous, play/pause and next on the small bar.">
              <Toggle label="Show play controls" checked={o.showControls} onChange={(v) => setOptions({ showControls: v })} />
            </Row>
            <Row label="Show shuffle and repeat" hint="Next to the play controls.">
              <Toggle label="Show shuffle and repeat" checked={o.showShuffleRepeat} onChange={(v) => setOptions({ showShuffleRepeat: v })} />
            </Row>
            <Row label="Show playlist button" hint="A folder button that lists your playlists; pick one to play it.">
              <Toggle label="Show playlist button" checked={o.showPlaylists} onChange={(v) => setOptions({ showPlaylists: v })} />
            </Row>
            <Row label="Show the seek bar" hint="Along the bottom of the small bar. Click it to jump.">
              <Toggle label="Show the seek bar" checked={o.showProgress} onChange={(v) => setOptions({ showProgress: v })} />
            </Row>

            <div>
              <span className="block text-[13px]">Colours</span>
              <div className="mt-2.5 flex flex-wrap items-center gap-2">
                <PaletteChip label="From the cover" selected={o.colors === "cover"} onClick={() => setOptions({ colors: "cover" })} />
                {ISLAND_PALETTES.map((p) => (
                  <PaletteChip key={p.id} label={p.label} palette={p.p} selected={o.colors === p.id} onClick={() => setOptions({ colors: p.id })} />
                ))}
                <PaletteChip
                  label="My colours"
                  palette={o.customColors as Palette}
                  selected={o.colors === "custom"}
                  onClick={() => setOptions({ colors: "custom" })}
                />
              </div>
              {o.colors === "custom" && (
                <div className="mt-3 flex items-center gap-2.5 text-[12.5px] text-muted">
                  My colours
                  {o.customColors.map((c, i) => (
                    <ColorInput
                      key={i}
                      label={`Colour ${i + 1}`}
                      value={c}
                      onChange={(v) => setOptions({ customColors: o.customColors.map((x, j) => (j === i ? v : x)) })}
                    />
                  ))}
                </div>
              )}
            </div>
          </>
        )}
      </div>
    </Section>
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

function PaletteChip({ label, palette, selected, onClick }: { label: string; palette?: Palette; selected: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      aria-pressed={selected}
      className={`flex items-center gap-2 rounded-full py-1 pl-1 pr-3 text-[12px] font-medium transition-colors ${
        selected ? "bg-accent/10 ring-2 ring-accent" : "ring-1 ring-line hover:bg-hover"
      }`}
    >
      <span
        className="h-5 w-8 rounded-full"
        style={{
          background: palette
            ? `linear-gradient(115deg, ${palette[0]}, ${palette[1]}, ${palette[2]})`
            : "conic-gradient(from 90deg, #f97316, #ec4899, #8b5cf6, #3b82f6, #22c55e, #f97316)",
        }}
      />
      {label}
      {selected && <Check size={13} className="text-accent" />}
    </button>
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
