import { MIN_LENGTHS, useLibrary, type MinLength } from "../store/library";
import { SortSelect } from "./ViewHeader";

const LABELS: Record<MinLength, string> = { 0: "Any", 10: "10 sec", 20: "20 sec", 30: "30 sec", 60: "1 min" };

/** The library-wide short-track filter; changing it here updates every view. */
export function MinLengthSelect() {
  const minLength = useLibrary((s) => s.minLength);
  const setMinLength = useLibrary((s) => s.setMinLength);
  return (
    <span title="Hide tracks shorter than this (and ones with an unknown length) in Songs, Tracks, Folders, Search and playlists">
      <SortSelect<string>
        label="Min length"
        value={String(minLength)}
        onChange={(v) => setMinLength(Number(v) as MinLength)}
        options={MIN_LENGTHS.map((m) => ({ value: String(m), label: LABELS[m] }))}
      />
    </span>
  );
}

/** " · 12 short tracks hidden" suffix for view subtitles. */
export function HiddenNote({ count }: { count: number }) {
  if (count <= 0) return null;
  return (
    <span className="text-faint">
      {" "}
      · {count.toLocaleString()} short {count === 1 ? "track" : "tracks"} hidden
    </span>
  );
}
