import type { ReactNode } from "react";
import { TrackRow } from "../../components/TrackRow";
import { rowHeight, useListStyle, type ListStyle } from "../../store/listStyle";
import { usePlayer } from "../../store/player";
import { Section, Segmented, Toggle } from "../SettingsView";

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4">
      <span className="text-[13.5px]">{label}</span>
      {children}
    </div>
  );
}

/** Android: how songs and the app's surfaces look, with a live example. */
export function SongListSection() {
  const style = useListStyle();
  const update = style.update;
  const sample = usePlayer((s) => s.status?.track ?? s.queue.items[0]?.track ?? null);
  const flag = (key: "cover" | "artist" | "bold" | "quality" | "playerQuality" | "glass", label: string) => (
    <Row label={label}>
      <Toggle label={label} checked={style[key]} onChange={(v) => update({ [key]: v })} />
    </Row>
  );
  return (
    <Section title="Song list" description="Choose how much each song shows.">
      <div className="space-y-4">
        {sample && (
          <div className="-mx-1 overflow-hidden rounded-lg bg-bg/60 ring-1 ring-line" style={{ height: rowHeight(style) }}>
            <TrackRow track={sample} onMenu={(e) => e.preventDefault()} />
          </div>
        )}
        <Row label="Row size">
          <Segmented<ListStyle["size"]>
            value={style.size}
            onChange={(size) => update({ size })}
            options={[
              { value: "compact", label: "Small" },
              { value: "normal", label: "Medium" },
              { value: "large", label: "Large" },
            ]}
          />
        </Row>
        {flag("cover", "Show covers")}
        {flag("artist", "Show artist")}
        {flag("bold", "Bold song names")}
        {flag("quality", "Show quality on each song")}
        {flag("playerQuality", "Show quality on the player")}
        {flag("glass", "Glass effect")}
      </div>
    </Section>
  );
}
