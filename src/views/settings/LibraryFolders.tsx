import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpen, FolderPlus, RefreshCw, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../../lib/api";
import { isMobile } from "../../lib/platform";
import type { LibraryFolder } from "../../lib/types";
import { useLibrary } from "../../store/library";
import { usePlayer } from "../../store/player";
import { Section } from "../SettingsView";
import { ScanBanner } from "../../components/ScanBanner";

export function LibraryFoldersSection() {
  const [folders, setFolders] = useState<LibraryFolder[]>([]);
  const scanning = useLibrary((s) => s.scanning);
  const trackCount = useLibrary((s) => s.allTracks.length);
  const toast = usePlayer((s) => s.pushToast);

  useEffect(() => {
    api.getLibraryFolders().then(setFolders);
  }, [scanning, trackCount]);

  const add = async () => {
    let picked: string | string[] | null;
    try {
      // Android: the system folder picker (after asking for access to music and audio).
      picked = isMobile ? await api.pickMusicFolder() : await open({ directory: true, multiple: true, title: "Add music folder" });
    } catch (e) {
      toast("error", String(e));
      return;
    }
    if (!picked) return;
    for (const p of Array.isArray(picked) ? picked : [picked]) {
      try {
        setFolders(await api.addLibraryFolder(p));
      } catch (e) {
        toast("error", String(e));
      }
    }
  };

  const remove = async (f: LibraryFolder) => {
    try {
      setFolders(await api.removeLibraryFolder(f.id));
    } catch (e) {
      toast("error", String(e));
    }
  };

  return (
    <Section
      title="Library folders"
      description="Folders are scanned recursively. Only new or changed files are re-read, so rescans are fast. Music files are never modified."
    >
      <div className="overflow-hidden rounded-lg ring-1 ring-line">
        {folders.length === 0 && <div className="px-4 py-4 text-[12.5px] text-faint">No folders yet.</div>}
        {folders.map((f) => (
          <div key={f.id} className="flex items-center gap-3 border-b border-line px-4 py-2.5 last:border-b-0">
            <FolderOpen size={16} className="shrink-0 text-accent" />
            <div className="min-w-0 flex-1">
              <div className="truncate text-[13px]" title={f.path}>
                {f.path}
              </div>
              <div className="text-[11.5px] text-faint">{f.trackCount.toLocaleString()} tracks</div>
            </div>
            <button
              aria-label={`Remove ${f.path}`}
              title="Remove from library"
              onClick={() => remove(f)}
              className="rounded-md p-1.5 text-faint hover:bg-hover hover:text-[#ef5b5b]"
            >
              <Trash2 size={15} />
            </button>
          </div>
        ))}
      </div>
      <div className="mt-3 flex flex-wrap gap-2">
        <button
          onClick={add}
          className="flex items-center gap-2 rounded-full bg-fg px-4 py-1.5 text-[12.5px] font-semibold text-bg hover:opacity-90"
        >
          <FolderPlus size={14} /> Add folder
        </button>
        <button
          onClick={() => api.rescanLibrary(false)}
          disabled={scanning}
          className="flex items-center gap-2 rounded-full px-4 py-1.5 text-[12.5px] font-medium ring-1 ring-line hover:bg-hover disabled:opacity-50"
        >
          <RefreshCw size={14} className={scanning ? "animate-spin" : ""} /> Rescan
        </button>
        <button
          onClick={() => api.rescanLibrary(true)}
          disabled={scanning}
          title="Re-read every file's tags and artwork"
          className="rounded-full px-4 py-1.5 text-[12.5px] font-medium text-muted ring-1 ring-line hover:bg-hover disabled:opacity-50"
        >
          Full rescan
        </button>
      </div>
      <div className="-mx-8">
        <ScanBanner />
      </div>
    </Section>
  );
}
