import { ArrowDownToLine, Download, FolderTree, Heart, LayoutGrid, ListMusic, ListOrdered, Music2, Plus, Settings2, SlidersHorizontal } from "lucide-react";
import type { ReactNode } from "react";
import { api } from "../lib/api";
import { usePlayer } from "../store/player";
import { useUi, type View } from "../store/ui";

/** Where the Donate button points: the developer's page on Kisa (kisa.ge), a Georgian donation site. */
export const DONATE_URL = "https://www.kisa.ge/donate/zhg15pbjbz";
import { openPlaylistEditor } from "./PlaylistEditor";
import { PlaylistNav } from "./PlaylistNav";
import { useDownloadsBadge } from "./DownloadsPanel";

function NavItem({
  icon,
  label,
  view,
  match,
  trailing,
}: {
  icon: ReactNode;
  label: string;
  view: View;
  match: View["kind"][];
  trailing?: ReactNode;
}) {
  const current = useUi((s) => s.view);
  const nowPlaying = useUi((s) => s.nowPlaying);
  const setView = useUi((s) => s.setView);
  const active = !nowPlaying && match.includes(current.kind);
  return (
    <div
      onClick={() => setView(view)}
      className={`group flex w-full cursor-pointer items-center gap-3 rounded-lg px-3 py-2 text-[13.5px] transition-colors duration-200 ${
        active ? "bg-hover font-semibold text-fg" : "text-muted hover:text-fg"
      }`}
    >
      <span className={`transition-colors duration-200 ${active ? "text-accent" : ""}`}>{icon}</span>
      <span className="flex-1">{label}</span>
      {trailing}
    </div>
  );
}

/** Opens the Downloads panel (Soulseek searches and downloads), with a count of active work. */
function DownloadsItem() {
  const open = useUi((s) => s.downloadsOpen);
  const toggle = useUi((s) => s.toggleDownloads);
  const badge = useDownloadsBadge();
  return (
    <div
      onClick={toggle}
      role="button"
      aria-pressed={open}
      className={`group flex w-full cursor-pointer items-center gap-3 rounded-lg px-3 py-2 text-[13.5px] transition-colors duration-200 ${
        open ? "bg-hover font-semibold text-fg" : "text-muted hover:text-fg"
      }`}
    >
      <span className={`transition-colors duration-200 ${open ? "text-accent" : ""}`}>
        <ArrowDownToLine size={18} />
      </span>
      <span className="flex-1">Downloads</span>
      {badge > 0 && (
        <span className="flex h-5 min-w-5 items-center justify-center rounded-full bg-accent px-1.5 text-[11px] font-bold text-black">
          {badge > 99 ? "99+" : badge}
        </span>
      )}
    </div>
  );
}

export function Sidebar() {
  return (
    <nav className="skin-glass np-hide flex w-[236px] shrink-0 flex-col overflow-hidden rounded-xl bg-elev" aria-label="Main">
      <div className="flex flex-col gap-0.5 px-2 pt-3">
        <NavItem icon={<LayoutGrid size={18} />} label="Songs" view={{ kind: "songs" }} match={["songs"]} />
        <NavItem icon={<Music2 size={18} />} label="Tracks" view={{ kind: "tracks" }} match={["tracks", "search"]} />
        <NavItem
          icon={<ListMusic size={18} />}
          label="Playlists"
          view={{ kind: "playlists" }}
          match={["playlists"]}
          trailing={
            <button
              aria-label="Create playlist"
              title="Create playlist"
              onClick={(e) => {
                e.stopPropagation();
                openPlaylistEditor(null);
              }}
              className="rounded-full p-1 text-muted opacity-0 transition-all duration-200 hover:bg-hover hover:text-fg group-hover:opacity-100"
            >
              <Plus size={15} />
            </button>
          }
        />
        <NavItem icon={<FolderTree size={18} />} label="Folders" view={{ kind: "folders" }} match={["folders"]} />
        <NavItem icon={<Download size={18} />} label="Sources" view={{ kind: "sources" }} match={["sources"]} />
      </div>

      <div className="mx-4 my-2 h-px bg-line/70" />

      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
        <PlaylistNav />
      </div>

      <div className="flex flex-col gap-0.5 border-t border-line/60 px-2 py-2">
        <DownloadsItem />
        <NavItem icon={<ListOrdered size={18} />} label="Queue" view={{ kind: "queue" }} match={["queue"]} />
        <NavItem icon={<SlidersHorizontal size={18} />} label="Equalizer" view={{ kind: "eq" }} match={["eq"]} />
        <NavItem icon={<Settings2 size={18} />} label="Settings" view={{ kind: "settings" }} match={["settings"]} />
        <button
          onClick={() => api.openUrl(DONATE_URL).catch((e) => usePlayer.getState().pushToast("error", String(e)))}
          title={`Support Undertone (${DONATE_URL.replace("https://", "")})`}
          className="group mt-1 flex w-full items-center gap-3 rounded-lg px-3 py-2 text-[13.5px] text-muted ring-1 ring-line/70 transition-colors duration-200 hover:bg-hover hover:text-fg"
        >
          <Heart size={18} className="text-[#ef5b8a] transition-transform duration-200 group-hover:scale-110" />
          <span className="flex-1 text-left">Donate</span>
        </button>
      </div>
    </nav>
  );
}
