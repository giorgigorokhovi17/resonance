import { ArrowDownToLine, BarChart3, ChevronLeft, ChevronRight, Download, FolderTree, Heart, ListMusic, ListOrdered, Menu, Music2, Pause, Play, Search, Settings2, SkipForward, SlidersHorizontal, X } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { api } from "../lib/api";
import { displayArtist } from "../lib/format";
import { useLiveCover, useLiveMotion } from "../store/library";
import { livePosition, usePlayer } from "../store/player";
import { useUi, type View } from "../store/ui";
import { useMenu } from "./ContextMenu";
import { Cover } from "./Cover";
import { useDownloadsBadge } from "./DownloadsPanel";
import { DONATE_URL } from "./Sidebar";
import { ViewHeader } from "./ViewHeader";

const TABS: { label: string; icon: ReactNode; view: View; match: View["kind"][] }[] = [
  { label: "Songs", icon: <Music2 size={22} />, view: { kind: "songs" }, match: ["songs"] },
  { label: "Playlists", icon: <ListMusic size={22} />, view: { kind: "playlists" }, match: ["playlists", "playlist", "review"] },
  { label: "Search", icon: <Search size={22} />, view: { kind: "search", q: "" }, match: ["search"] },
  { label: "Sources", icon: <Download size={22} />, view: { kind: "sources" }, match: ["sources"] },
  { label: "More", icon: <Menu size={22} />, view: { kind: "more" }, match: ["more", "folders", "queue", "eq", "stats", "settings", "tracks"] },
];

/** The views with a tab of their own need no way back. */
const isTopLevel = (v: View) => ["songs", "playlists", "search", "sources", "more"].includes(v.kind);

export function BottomTabs() {
  const view = useUi((s) => s.view);
  const setView = useUi((s) => s.setView);
  const badge = useDownloadsBadge();
  return (
    <nav className="skin-clear flex h-[60px] shrink-0 items-stretch border-t border-line/60 bg-bg" aria-label="Main">
      {TABS.map((t) => {
        const on = t.match.includes(view.kind);
        return (
          <button
            key={t.label}
            aria-current={on ? "page" : undefined}
            data-downloads-target={t.label === "Sources" || undefined}
            onClick={() => setView(t.view)}
            className={`relative flex flex-1 flex-col items-center justify-center gap-0.5 text-[11px] font-medium ${on ? "text-accent" : "text-muted"}`}
          >
            {t.icon}
            {t.label}
            {t.label === "Sources" && badge > 0 && (
              <span className="absolute left-1/2 top-1.5 ml-1.5 flex h-4 min-w-4 items-center justify-center rounded-full bg-accent px-1 text-[10px] font-bold text-on-accent">
                {badge > 99 ? "99+" : badge}
              </span>
            )}
          </button>
        );
      })}
    </nav>
  );
}

function MiniProgress() {
  const duration = usePlayer((s) => s.status?.duration ?? 0);
  const bar = useRef<HTMLDivElement>(null);
  useEffect(() => {
    let raf = 0;
    const tick = () => {
      if (bar.current) bar.current.style.width = `${duration > 0 ? Math.min(100, (livePosition() / duration) * 100) : 0}%`;
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [duration]);
  return (
    <div className="absolute inset-x-3 bottom-0 h-0.5 overflow-hidden rounded-full bg-fg/15">
      <div ref={bar} className="h-full bg-fg" />
    </div>
  );
}

/** The song playing, above the tabs. Tap it for the full player. */
export function MiniPlayer() {
  const track = usePlayer((s) => s.status?.track ?? s.queue.items.find((i) => i.uid === s.queue.currentUid)?.track ?? null);
  const playing = usePlayer((s) => s.status?.state === "playing");
  const coverId = useLiveCover(track);
  const motionId = useLiveMotion(track);
  const setNowPlaying = useUi((s) => s.setNowPlaying);
  if (!track) return null;
  return (
    <div className="skin-clear shrink-0 bg-bg px-2 pt-2">
      <div onClick={() => setNowPlaying(true)} className="relative flex h-14 items-center gap-3 overflow-hidden rounded-xl bg-panel pl-2 pr-1 ring-1 ring-line">
        <Cover coverId={coverId} motionId={motionId} motion="tile" className="h-10 w-10" rounded="rounded-md" iconSize={18} />
        <div className="min-w-0 flex-1">
          <div className="truncate text-[14px] font-medium">{track.title}</div>
          <div className="truncate text-[12.5px] text-muted">{displayArtist(track)}</div>
        </div>
        <button
          aria-label={playing ? "Pause" : "Play"}
          onClick={(e) => {
            e.stopPropagation();
            api.togglePlay();
          }}
          className="flex h-11 w-11 shrink-0 items-center justify-center"
        >
          {playing ? <Pause size={24} fill="currentColor" strokeWidth={0} /> : <Play size={24} fill="currentColor" strokeWidth={0} />}
        </button>
        <button
          aria-label="Next"
          onClick={(e) => {
            e.stopPropagation();
            api.next();
          }}
          className="flex h-11 w-11 shrink-0 items-center justify-center"
        >
          <SkipForward size={22} fill="currentColor" />
        </button>
        <MiniProgress />
      </div>
    </div>
  );
}

/** Search box on the Search tab, or the way back on a page without a tab. */
export function MobileTopBar() {
  const view = useUi((s) => s.view);
  if (view.kind === "search") return <SearchBar />;
  if (isTopLevel(view)) return null;
  return (
    <div className="flex h-11 shrink-0 items-center px-1">
      <button aria-label="Back" onClick={() => useUi.getState().goBack()} className="flex h-11 w-11 items-center justify-center text-fg">
        <ChevronLeft size={26} />
      </button>
    </div>
  );
}

function SearchBar() {
  const view = useUi((s) => s.view);
  const setView = useUi((s) => s.setView);
  const [q, setQ] = useState(view.kind === "search" ? view.q : "");
  useEffect(() => {
    // Replace while typing so Back leaves Search rather than going through every letter.
    const h = setTimeout(() => setView({ kind: "search", q }, true), 140);
    return () => clearTimeout(h);
  }, [q]);
  return (
    <div className="relative shrink-0 px-3 pb-1 pt-2">
      <Search size={17} className="pointer-events-none absolute left-6 top-1/2 mt-0.5 -translate-y-1/2 text-faint" />
      <input
        autoFocus
        value={q}
        onChange={(e) => setQ(e.target.value)}
        placeholder="Songs, artists, albums"
        spellCheck={false}
        className="h-11 w-full rounded-xl bg-panel pl-10 pr-10 text-fg outline-none ring-1 ring-line placeholder:text-faint focus:ring-accent"
      />
      {q && (
        <button aria-label="Clear search" onClick={() => setQ("")} className="absolute right-3 top-1/2 mt-0.5 flex h-10 w-10 -translate-y-1/2 items-center justify-center text-faint">
          <X size={17} />
        </button>
      )}
    </div>
  );
}

function MoreRow({ icon, label, onClick, trailing, chevron = true }: { icon: ReactNode; label: string; onClick: () => void; trailing?: ReactNode; chevron?: boolean }) {
  return (
    <button onClick={onClick} className="flex h-14 w-full items-center gap-4 px-5 text-left text-[15px] active:bg-hover">
      <span className="text-muted">{icon}</span>
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {trailing}
      {chevron && <ChevronRight size={18} className="text-faint" />}
    </button>
  );
}

/** Everything that doesn't get a bottom tab. */
export function MoreView() {
  const setView = useUi((s) => s.setView);
  const toggleDownloads = useUi((s) => s.toggleDownloads);
  const badge = useDownloadsBadge();
  return (
    <div className="h-full overflow-y-auto">
      <ViewHeader title="More" />
      <MoreRow
        icon={<ArrowDownToLine size={20} />}
        label="Downloads"
        onClick={toggleDownloads}
        trailing={badge > 0 ? <span className="rounded-full bg-accent px-2 text-[12px] font-bold text-on-accent">{badge}</span> : undefined}
      />
      <MoreRow icon={<ListOrdered size={20} />} label="Queue" onClick={() => setView({ kind: "queue" })} />
      <MoreRow icon={<FolderTree size={20} />} label="Folders" onClick={() => setView({ kind: "folders" })} />
      <MoreRow icon={<SlidersHorizontal size={20} />} label="Equalizer" onClick={() => setView({ kind: "eq" })} />
      <MoreRow icon={<BarChart3 size={20} />} label="Your stats" onClick={() => setView({ kind: "stats" })} />
      <MoreRow icon={<Settings2 size={20} />} label="Settings" onClick={() => setView({ kind: "settings" })} />
      <div className="mx-5 my-2 h-px bg-line" />
      <MoreRow
        icon={<Heart size={20} className="text-[#ef5b8a]" />}
        label="Support Undertone"
        chevron={false}
        onClick={() => api.openUrl(DONATE_URL).catch((e) => usePlayer.getState().pushToast("error", String(e)))}
      />
    </div>
  );
}

/**
 * The phone's Back button. The WebView handles Back through its history, so an entry is kept there
 * while there's something to go back to; with none, Back leaves the app as usual.
 */
export function useBackButton() {
  useEffect(() => {
    let armed = false;
    const canGoBack = () => {
      const u = useUi.getState();
      return !!useMenu.getState().open || u.lyricsOpen || u.queueOpen || u.downloadsOpen || u.nowPlaying || u.back.length > 0 || !!document.querySelector("[aria-modal=true]");
    };
    const arm = () => {
      if (!armed && canGoBack()) {
        armed = true;
        history.pushState(null, "");
      }
    };
    const onBack = () => {
      armed = false;
      const u = useUi.getState();
      const dialog = document.querySelector("[aria-modal=true]");
      if (useMenu.getState().open) useMenu.getState().close();
      else if (dialog) dialog.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
      else if (u.lyricsOpen) u.setLyricsOpen(false);
      else if (u.queueOpen) u.toggleQueue();
      else if (u.downloadsOpen) u.toggleDownloads();
      else if (u.nowPlaying) u.setNowPlaying(false);
      else u.goBack();
      arm();
    };
    window.addEventListener("popstate", onBack);
    const unUi = useUi.subscribe(arm);
    const unMenu = useMenu.subscribe(arm);
    return () => {
      window.removeEventListener("popstate", onBack);
      unUi();
      unMenu();
    };
  }, []);
}
