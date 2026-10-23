import { useCallback, useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "./lib/api";
import { initPlayer, usePlayer } from "./store/player";
import { initLibrary, useLiveCover } from "./store/library";
import { initPlaylists, usePlaylists } from "./store/playlists";
import { initMedia, useMedia } from "./store/media";
import { useUi } from "./store/ui";
import { initUpdater } from "./store/updater";
import { initSoulseek } from "./store/soulseek";
import { initSleep } from "./store/sleep";
import { TitleBar } from "./components/TitleBar";
import { Sidebar } from "./components/Sidebar";
import { PlayerBar } from "./components/PlayerBar";
import { Toasts } from "./components/Toasts";
import { ContextMenuHost } from "./components/ContextMenu";
import { NavSearch } from "./components/SearchBox";
import { DialogHost } from "./components/Dialog";
import { DownloadsPanel } from "./components/DownloadsPanel";
import { LinkImportHost } from "./components/LinkImportDialog";
import { PlaylistEditorHost } from "./components/PlaylistEditor";
import { LyricsDialog } from "./components/LyricsPanel";
import { Ambient } from "./components/Ambient";
import { Backdrop } from "./components/Backdrop";
import { SettingsView, showSettingsTab } from "./views/SettingsView";
import { SongsView } from "./views/SongsView";
import { TracksView } from "./views/TracksView";
import { FoldersView } from "./views/FoldersView";
import { SearchView } from "./views/SearchView";
import { QueuePanel, QueueView } from "./views/QueueView";
import { PlaylistsView } from "./views/PlaylistsView";
import { PlaylistView } from "./views/PlaylistView";
import { ReviewView } from "./views/ReviewView";
import { EqView } from "./views/EqView";
import { SourcesView } from "./views/SourcesView";
import { StatsView } from "./views/StatsView";
import { NowPlaying } from "./views/NowPlaying";
import { useShortcuts } from "./hooks/useShortcuts";
import { useCoverColor } from "./hooks/useCoverColor";
import { useAppearance } from "./hooks/useAppearance";

const AUDIO_EXT = ["flac", "wav", "wave", "mp3", "m4a", "mp4", "aac", "ogg", "oga", "opus", "aif", "aiff", "aifc", "caf", "mka"];

export async function openFiles() {
  const picked = await open({
    multiple: true,
    title: "Play audio files",
    filters: [{ name: "Audio", extensions: AUDIO_EXT }],
  });
  if (!picked) return;
  const paths = Array.isArray(picked) ? picked : [picked];
  try {
    await api.playFiles(paths);
  } catch (e) {
    usePlayer.getState().pushToast("error", String(e));
  }
}

/**
 * Colors taken from artwork: `ambient` for the backdrop (the open playlist's cover, otherwise
 * the playing song's) and `playing` for the album-art accent option.
 */
function useArtColors() {
  const view = useUi((s) => s.view);
  const playlists = usePlaylists((s) => s.playlists);
  const track = usePlayer(
    (s) => s.status?.track ?? s.queue.items.find((i) => i.uid === s.queue.currentUid)?.track ?? null,
  );
  const playingCover = useLiveCover(track);
  let coverId: string | null | undefined = playingCover;
  if (view.kind === "playlist" || view.kind === "review") {
    const p = playlists.find((x) => x.id === view.id);
    coverId = p?.coverId ?? p?.previewCovers[0] ?? playingCover;
  }
  return { ambient: useCoverColor(coverId), playing: useCoverColor(playingCover) };
}

/**
 * Custom backgrounds that apply now: the app's (Settings → Visual, when it covers the whole app)
 * and the open playlist's page background.
 */
function useBackgrounds() {
  const view = useUi((s) => s.view);
  const skin = usePlayer((s) => s.settings?.appearance.background ?? null);
  const scope = usePlayer((s) => s.settings?.appearance.backgroundScope ?? "app");
  const playlistBg = usePlaylists((s) => (view.kind === "playlist" ? s.playlists.find((p) => p.id === view.id)?.background : null) ?? null);
  const known = useMedia((s) => s.byId);
  return {
    skin: skin && scope === "app" && known.has(skin.mediaId) ? skin : null,
    page: playlistBg && known.has(playlistBg.mediaId) ? playlistBg : null,
  };
}

function MainView() {
  const view = useUi((s) => s.view);
  switch (view.kind) {
    case "songs":
      return <SongsView />;
    case "tracks":
      return <TracksView />;
    case "folders":
      return <FoldersView path={view.path} />;
    case "search":
      return <SearchView q={view.q} />;
    case "queue":
      return <QueueView />;
    case "playlists":
      return <PlaylistsView />;
    case "playlist":
      return <PlaylistView id={view.id} />;
    case "review":
      return <ReviewView id={view.id} />;
    case "eq":
      return <EqView />;
    case "sources":
      return <SourcesView />;
    case "stats":
      return <StatsView />;
    case "settings":
      return (
        <div className="h-full overflow-y-auto">
          <SettingsView />
        </div>
      );
  }
}

export default function App() {
  const searchRef = useRef<HTMLInputElement>(null);
  const viewKey = useUi((s) => JSON.stringify(s.view));
  const art = useArtColors();
  useAppearance(art.playing);
  const showAmbient = usePlayer((s) => s.settings?.appearance.ambient ?? true);
  const backgrounds = useBackgrounds();
  const nowPlaying = useUi((s) => s.nowPlaying);

  // The app background shows through the main surfaces (index.css), and the library underneath
  // must not show through Now Playing.
  useEffect(() => {
    const el = document.documentElement;
    el.toggleAttribute("data-skin", !!backgrounds.skin);
    el.toggleAttribute("data-np-over-skin", !!backgrounds.skin && nowPlaying);
  }, [backgrounds.skin, nowPlaying]);

  useEffect(() => {
    initPlayer().catch((e) => console.error(e));
    initLibrary().catch((e) => usePlayer.getState().pushToast("error", `Library failed to load: ${e}`));
    initPlaylists().catch((e) => console.error(e));
    initMedia().catch((e) => console.error(e));
    initUpdater();
    initSoulseek().catch((e) => console.error(e));
    initSleep().catch((e) => console.error(e));
    // Files dropped from Explorer start playing.
    const un = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === "drop" && e.payload.paths.length) {
        api.playFiles(e.payload.paths).catch((err) => usePlayer.getState().pushToast("error", String(err)));
      }
    });
    // "Dynamic Island settings" from the tray or the island's menu.
    const unIsland = listen("ui:island-settings", () => {
      showSettingsTab("general");
      useUi.getState().setView({ kind: "settings" });
    });
    return () => {
      un.then((f) => f());
      unIsland.then((f) => f());
    };
  }, []);

  const focusSearch = useCallback(() => {
    searchRef.current?.focus();
    searchRef.current?.select();
  }, []);
  useShortcuts(focusSearch);

  return (
    <div className={`relative isolate flex h-full flex-col text-fg ${backgrounds.skin ? "" : "bg-bg"}`}>
      {backgrounds.skin && <Backdrop backdrop={backgrounds.skin} place="app" scrim="bg" className="absolute inset-0 -z-10" />}
      <TitleBar>
        <NavSearch ref={searchRef} />
      </TitleBar>
      <div className="relative flex min-h-0 flex-1 gap-2 px-2">
        <Sidebar />
        <main className="skin-glass np-hide relative min-w-0 flex-1 overflow-hidden rounded-xl bg-elev">
          {backgrounds.page ? (
            <Backdrop backdrop={backgrounds.page} place="page" scrim="elev" />
          ) : (
            <Ambient color={showAmbient && !backgrounds.skin ? art.ambient : null} />
          )}
          <div key={viewKey} className="relative h-full animate-in">
            <MainView />
          </div>
          <QueuePanel />
          <DownloadsPanel />
        </main>
        <NowPlaying />
      </div>
      <PlayerBar />
      <Toasts />
      <ContextMenuHost />
      <DialogHost />
      <LinkImportHost />
      <PlaylistEditorHost />
      <LyricsDialog />
    </div>
  );
}
