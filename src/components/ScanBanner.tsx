import { Loader2 } from "lucide-react";
import { useLibrary } from "../store/library";

const PHASE: Record<string, string> = {
  walking: "Looking for music",
  reading: "Reading tags",
  artwork: "Extracting album art",
  saving: "Saving",
};

/** Slim progress banner shown while the library is being scanned. */
export function ScanBanner() {
  const scanning = useLibrary((s) => s.scanning);
  const p = useLibrary((s) => s.progress);
  if (!scanning) return null;
  const frac = p && p.total > 0 ? p.done / p.total : null;
  return (
    <div className="mx-8 mt-4 flex items-center gap-3 rounded-lg bg-panel px-4 py-2.5 text-[12.5px] ring-1 ring-line animate-in">
      <Loader2 size={15} className="shrink-0 animate-spin text-accent" />
      <span className="shrink-0 font-medium">{p ? PHASE[p.phase] ?? "Scanning" : "Scanning library"}</span>
      {p && p.total > 0 && (
        <span className="tabular shrink-0 text-muted">
          {p.done.toLocaleString()} / {p.total.toLocaleString()}
        </span>
      )}
      <span className="min-w-0 flex-1 truncate text-faint">{p?.folder}</span>
      {frac !== null && (
        <div className="h-1 w-32 shrink-0 overflow-hidden rounded-full bg-fg/10">
          <div className="accent-gradient h-full" style={{ width: `${frac * 100}%` }} />
        </div>
      )}
    </div>
  );
}
