import { open, save } from "@tauri-apps/plugin-dialog";
import { ArchiveRestore, Download } from "lucide-react";
import { useState } from "react";
import { confirmDialog } from "../../components/Dialog";
import { SecondaryButton } from "../../components/EmptyState";
import { api } from "../../lib/api";
import type { RestoreSummary } from "../../lib/types";
import { usePlayer } from "../../store/player";
import { Section } from "../SettingsView";

const EXT = "resonance-backup";

/** Frontend preferences (sort orders, filters, chosen headphones…) to carry in the backup. */
function uiPrefs(): Record<string, string> {
  const out: Record<string, string> = {};
  try {
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i);
      if (k) out[k] = localStorage.getItem(k) ?? "";
    }
  } catch {
    /* ignore */
  }
  return out;
}

function restoreUiPrefs(ui: Record<string, string> | null) {
  if (!ui) return;
  try {
    for (const [k, v] of Object.entries(ui)) localStorage.setItem(k, v);
  } catch {
    /* ignore */
  }
}

const plural = (n: number, word: string, many = `${word}s`) => `${n.toLocaleString()} ${n === 1 ? word : many}`;

export function BackupSection() {
  const [busy, setBusy] = useState<"export" | "import" | null>(null);
  const [result, setResult] = useState<RestoreSummary | null>(null);
  const toast = usePlayer.getState().pushToast;

  const doExport = async () => {
    const date = new Date().toISOString().slice(0, 10);
    const path = await save({
      title: "Save Undertone backup",
      defaultPath: `Undertone ${date}.${EXT}`,
      filters: [{ name: "Undertone backup", extensions: [EXT] }],
    });
    if (!path) return;
    setBusy("export");
    try {
      const s = await api.exportBackup(path, uiPrefs());
      const media = s.media ? `, ${plural(s.media, "custom media file")}` : "";
      const left = s.mediaSkipped ? ` ${plural(s.mediaSkipped, "large media file was", "large media files were")} left out.` : "";
      toast(
        "info",
        `Backup saved: ${plural(s.playlists, "playlist")}, ${plural(s.presets, "EQ preset")}, ${plural(s.artwork, "artwork image")}${media} (${(s.bytes / 1048576).toFixed(1)} MB).${left}`,
      );
    } catch (e) {
      toast("error", String(e));
    } finally {
      setBusy(null);
    }
  };

  const doImport = async () => {
    const path = await open({
      multiple: false,
      title: "Restore Undertone backup",
      filters: [{ name: "Undertone backup", extensions: [EXT, "json"] }],
    });
    if (!path || Array.isArray(path)) return;
    const ok = await confirmDialog(
      "Restore this backup?",
      "Playlists with the same name are replaced, EQ presets and settings are overwritten, and library folders from the backup are added if they exist on this PC. Your music files are not touched.",
      "Restore",
    );
    if (!ok) return;
    setBusy("import");
    try {
      const r = await api.importBackup(path);
      restoreUiPrefs(r.ui);
      setResult(r);
    } catch (e) {
      toast("error", String(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <Section
      title="Backup & restore"
      description="Save your playlists, EQ presets, custom artwork, library folders and settings to one file, then restore it on a new PC. Music files aren't included; if they live on a different drive or folder there, playlists are matched to them automatically. Custom GIFs, videos and background photos are included up to 25 MB each and 200 MB in all."
    >
      <div className="flex flex-wrap gap-2">
        <SecondaryButton onClick={doExport}>
          <Download size={15} /> {busy === "export" ? "Saving…" : "Back up…"}
        </SecondaryButton>
        <SecondaryButton onClick={doImport}>
          <ArchiveRestore size={15} /> {busy === "import" ? "Restoring…" : "Restore…"}
        </SecondaryButton>
      </div>

      {result && (
        <div className="mt-4 space-y-2 rounded-lg bg-bg p-4 text-[12.5px] leading-relaxed ring-1 ring-line">
          <div className="font-medium">
            Restored {plural(result.playlists, "playlist")}, {plural(result.presets, "EQ preset")} and{" "}
            {plural(result.artwork, "artwork image")}.
          </div>
          {result.foldersAdded.length > 0 && (
            <div className="text-muted">
              Added and scanning: <span className="text-fg">{result.foldersAdded.join(", ")}</span>
            </div>
          )}
          {result.relocated > 0 && (
            <div className="text-muted">{plural(result.relocated, "playlist song")} found in a different location on this PC.</div>
          )}
          {result.foldersMissing.length > 0 && (
            <div className="text-warn">
              Not on this PC: {result.foldersMissing.join(", ")}. Add the folder where that music is now (Library folders, above)
              and restore again to relink playlists.
            </div>
          )}
          {result.skippedMedia.length > 0 && (
            <div className="text-warn">
              Not restored (animated covers show their still frame instead):
              <ul className="ml-4 list-disc">
                {result.skippedMedia.map((m) => (
                  <li key={m}>{m}</li>
                ))}
              </ul>
            </div>
          )}
          {result.unresolved > 0 && (
            <div className="text-warn">{plural(result.unresolved, "playlist song")} couldn't be found yet and show as missing.</div>
          )}
          <SecondaryButton onClick={() => window.location.reload()}>Reload to apply everything</SecondaryButton>
        </div>
      )}
    </Section>
  );
}
