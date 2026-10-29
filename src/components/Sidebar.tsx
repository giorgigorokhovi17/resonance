import { ArrowDownToLine, BarChart3, Download, FolderTree, Heart, LayoutGrid, ListMusic, ListOrdered, Music2, Plus, Settings2, SlidersHorizontal } from "lucide-react";
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

/** A square icon button for the bottom row, which keeps the room above for playlists. */
function IconItem({
  icon,
  label,
  active,
  onClick,
  badge,
  target,
}: {
  icon: ReactNode;
  label: string;
  active: boolean;
  onClick: () => void;
  badge?: number;
  /** Where the fly-to-Downloads animation lands. */
  target?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      aria-label={label}
      aria-pressed={active}
      title={label}
      data-downloads-target={target || undefined}
      className={`relative flex h-9 flex-1 items-center justify-center rounded-lg transition-colors duration-200 ${
        active ? "bg-hover text-accent" : "text-muted hover:bg-hover/70 hover:text-fg"
      }`}
    >
      {icon}
      {!!badge && badge > 0 && (
        <span className="absolute -right-0.5 -top-0.5 flex h-4 min-w-4 items-center justify-center rounded-full bg-accent px-1 text-[10px] font-bold text-black">
          {badge > 99 ? "99+" : badge}
        </span>
      )}
    </button>
  );
}

/** Downloads, Queue, Equalizer, Settings and Donate as one row of icons. */
function BottomBar() {
  const view = useUi((s) => s.view);
  const nowPlaying = useUi((s) => s.nowPlaying);
  const setView = useUi((s) => s.setView);
  const downloadsOpen = useUi((s) => s.downloadsOpen);
  const toggleDownloads = useUi((s) => s.toggleDownloads);
  const badge = useDownloadsBadge();
  const on = (kind: View["kind"]) => !nowPlaying && view.kind === kind;
  return (
    <div className="flex items-center gap-1 border-t border-line/60 px-2 py-2">
      <IconItem
        icon={<ArrowDownToLine size={18} />}
        label={badge > 0 ? `Downloads (${badge} active)` : "Downloads"}
        active={downloadsOpen}
        onClick={toggleDownloads}
        badge={badge}
        target
      />
      <IconItem icon={<ListOrdered size={18} />} label="Queue" active={on("queue")} onClick={() => setView({ kind: "queue" })} />
      <IconItem icon={<SlidersHorizontal size={18} />} label="Equalizer" active={on("eq")} onClick={() => setView({ kind: "eq" })} />
      <IconItem icon={<Settings2 size={18} />} label="Settings" active={on("settings")} onClick={() => setView({ kind: "settings" })} />
      <button
        onClick={() => api.openUrl(DONATE_URL).catch((e) => usePlayer.getState().pushToast("error", String(e)))}
        aria-label="Donate"
        title={`Support Undertone (${DONATE_URL.replace("https://", "")})`}
        className="group flex h-9 flex-1 items-center justify-center rounded-lg ring-1 ring-line/70 transition-colors duration-200 hover:bg-hover"
      >
        <Heart size={18} className="text-[#ef5b8a] transition-transform duration-200 group-hover:scale-110" />
      </button>
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
        <NavItem icon={<BarChart3 size={18} />} label="Your stats" view={{ kind: "stats" }} match={["stats"]} />
      </div>

      <div className="mx-4 my-2 h-px bg-line/70" />

      <div className="min-h-0 flex-1">
        <PlaylistNav />
      </div>

      <BottomBar />
    </nav>
  );
}
