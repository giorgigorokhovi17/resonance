import { Download, ListMusic, Maximize2, Mic2, Moon, Pause, Play, Repeat, Repeat1, Shuffle, SkipBack, SkipForward, Volume1, Volume2, VolumeX } from "lucide-react";
import { useEffect, useState, type MouseEvent, type ReactNode } from "react";
import { api } from "../lib/api";
import { useDownloadsBadge } from "./DownloadsPanel";
import { displayArtist } from "../lib/format";
import { usePlayer } from "../store/player";
import { useUi } from "../store/ui";
import { useLiveCover, useLiveMotion } from "../store/library";
import { Cover } from "./Cover";
import { FormatBadge } from "./FormatBadge";
import { Seekbar } from "./Seekbar";
import { openMenu, SEP, type MenuItem } from "./ContextMenu";
import { setSleepTimer, sleepRemaining, useSleep } from "../store/sleep";

export function IconButton({
  children,
  onClick,
  label,
  active,
  size = "md",
  disabled,
}: {
  children: ReactNode;
  onClick?: (e: MouseEvent<HTMLButtonElement>) => void;
  label: string;
  active?: boolean;
  size?: "sm" | "md";
  disabled?: boolean;
}) {
  return (
    <button
      aria-label={label}
      title={label}
      disabled={disabled}
      onClick={onClick}
      className={`relative flex items-center justify-center rounded-full transition-colors disabled:opacity-30 ${
        size === "sm" ? "h-8 w-8" : "h-9 w-9"
      } ${active ? "text-accent" : "text-muted hover:text-fg"} hover:bg-hover`}
    >
      {children}
      {active && <span className="absolute bottom-0.5 h-1 w-1 rounded-full bg-accent" />}
    </button>
  );
}

export function PlayButton({ big = false }: { big?: boolean }) {
  const state = usePlayer((s) => s.status?.state);
  const playing = state === "playing";
  const sz = big ? "h-14 w-14" : "h-10 w-10";
  return (
    <button
      aria-label={playing ? "Pause" : "Play"}
      title={playing ? "Pause (Space)" : "Play (Space)"}
      onClick={() => api.togglePlay()}
      className={`${sz} flex items-center justify-center rounded-full bg-fg text-bg shadow-md transition-transform hover:scale-105 active:scale-95`}
    >
      {playing ? (
        <Pause size={big ? 24 : 18} fill="currentColor" strokeWidth={0} />
      ) : (
        <Play size={big ? 24 : 18} fill="currentColor" strokeWidth={0} className="translate-x-px" />
      )}
    </button>
  );
}

export function TransportControls({ big = false }: { big?: boolean }) {
  const queue = usePlayer((s) => s.queue);
  const RepeatIcon = queue.repeat === "one" ? Repeat1 : Repeat;
  const ic = big ? 20 : 17;
  return (
    <div className={`flex items-center ${big ? "gap-4" : "gap-2"}`}>
      <IconButton label="Shuffle (S)" active={queue.shuffle} onClick={() => api.setShuffle(!queue.shuffle)}>
        <Shuffle size={ic - 1} />
      </IconButton>
      <IconButton label="Previous (Ctrl+←)" onClick={() => api.previous()}>
        <SkipBack size={ic} fill="currentColor" />
      </IconButton>
      <PlayButton big={big} />
      <IconButton label="Next (Ctrl+→)" onClick={() => api.next()}>
        <SkipForward size={ic} fill="currentColor" />
      </IconButton>
      <IconButton
        label={`Repeat: ${queue.repeat} (R)`}
        active={queue.repeat !== "off"}
        onClick={() => api.setRepeat(queue.repeat === "off" ? "all" : queue.repeat === "all" ? "one" : "off")}
      >
        <RepeatIcon size={ic - 1} />
      </IconButton>
    </div>
  );
}

export function VolumeControl({ width = "w-28" }: { width?: string }) {
  const volume = usePlayer((s) => s.status?.volume ?? 0.8);
  const muted = usePlayer((s) => s.status?.muted ?? false);
  const setLocal = usePlayer((s) => s.setLocalVolume);
  const Icon = muted || volume === 0 ? VolumeX : volume < 0.5 ? Volume1 : Volume2;
  return (
    <div className="flex items-center gap-1">
      <IconButton label={muted ? "Unmute (M)" : "Mute (M)"} size="sm" onClick={() => api.setMuted(!muted)}>
        <Icon size={17} />
      </IconButton>
      <input
        type="range"
        min={0}
        max={1}
        step={0.005}
        value={volume}
        aria-label="Volume"
        title={`Volume ${Math.round(volume * 100)}%${volume >= 1 ? " (unity gain)" : ""}`}
        className={`slider ${width}`}
        style={{ ["--fill" as string]: `${volume * 100}%` }}
        onChange={(e) => {
          const v = parseFloat(e.target.value);
          setLocal(v);
          api.setVolume(v);
        }}
        onWheel={(e) => {
          const v = Math.min(1, Math.max(0, volume + (e.deltaY < 0 ? 0.03 : -0.03)));
          setLocal(v);
          api.setVolume(v);
        }}
      />
    </div>
  );
}

const SLEEP_MINUTES = [15, 30, 45, 60, 90];

function fmtLeft(secs: number) {
  const m = Math.ceil(secs / 60);
  return m >= 60 ? `${Math.floor(m / 60)}h${m % 60 ? ` ${m % 60}m` : ""}` : `${m}m`;
}

/** Sleep timer: pause after a while, or stop when this song ends. */
function SleepButton() {
  const status = useSleep((s) => s.status);
  const [left, setLeft] = useState(sleepRemaining);
  useEffect(() => {
    setLeft(sleepRemaining());
    if (status.mode !== "minutes") return;
    const id = window.setInterval(() => setLeft(sleepRemaining()), 1000);
    return () => window.clearInterval(id);
  }, [status]);
  const active = status.mode !== "off";
  const label =
    status.mode === "minutes" && left != null
      ? `Sleep timer: pauses in ${fmtLeft(left)}`
      : status.mode === "endOfSong"
        ? "Sleep timer: stops after this song"
        : "Sleep timer";
  const run = (p: Promise<void>) => p.catch((e) => usePlayer.getState().pushToast("error", String(e)));
  const items: MenuItem[] = [
    ...SLEEP_MINUTES.map((m) => ({
      label: m < 60 ? `${m} minutes` : m === 60 ? "1 hour" : `${m / 60} hours`,
      onClick: () => run(setSleepTimer(m)),
    })),
    { label: "End of this song", onClick: () => run(setSleepTimer(null, true)) },
    ...(active ? [SEP, { label: "Turn off", onClick: () => run(setSleepTimer(null)) }] : []),
  ];
  return (
    <div className="flex items-center">
      <IconButton label={label} active={active} onClick={(e) => openMenu(e, items)}>
        <Moon size={16} />
      </IconButton>
      {status.mode === "minutes" && left != null && (
        <span className="tabular -ml-0.5 mr-1 text-[11px] font-semibold text-accent" title={label}>
          {fmtLeft(left)}
        </span>
      )}
    </div>
  );
}

export function PlayerBar() {
  // When stopped (e.g. right after launch) show the restored queue's current track.
  const track = usePlayer(
    (s) => s.status?.track ?? s.queue.items.find((i) => i.uid === s.queue.currentUid)?.track ?? null,
  );
  const coverId = useLiveCover(track);
  const motionId = useLiveMotion(track);
  const view = useUi((s) => s.view);
  const setView = useUi((s) => s.setView);
  const toggleNowPlaying = useUi((s) => s.toggleNowPlaying);
  const nowPlaying = useUi((s) => s.nowPlaying);
  const queueOpen = useUi((s) => s.queueOpen);
  const toggleQueue = useUi((s) => s.toggleQueue);
  const downloadsOpen = useUi((s) => s.downloadsOpen);
  const toggleDownloads = useUi((s) => s.toggleDownloads);
  const downloadsBadge = useDownloadsBadge();
  const lyricsOpen = useUi((s) => s.lyricsOpen);
  const setLyricsOpen = useUi((s) => s.setLyricsOpen);

  return (
    <div className="skin-clear grid h-[84px] shrink-0 grid-cols-[minmax(0,1fr)_minmax(0,1.6fr)_minmax(0,1fr)] items-center gap-4 bg-bg px-4">
      <div className="flex min-w-0 items-center gap-3">
        <button
          className="group relative shrink-0"
          onClick={toggleNowPlaying}
          aria-label="Open now playing"
          title="Now playing (F)"
        >
          <Cover coverId={coverId} motionId={motionId} motion="tile" className="h-14 w-14 shadow-md" iconSize={22} />
          {track && (
            <span className="absolute inset-0 flex items-center justify-center rounded-md bg-black/45 opacity-0 transition-opacity group-hover:opacity-100">
              <Maximize2 size={16} className="text-white" />
            </span>
          )}
        </button>
        <div className="min-w-0 flex-1">
          {track ? (
            <>
              <div className="truncate text-[13.5px] font-semibold">{track.title}</div>
              <button
                className="block max-w-full truncate text-left text-[12.5px] text-muted hover:text-fg hover:underline"
                onClick={() => setView({ kind: "search", q: displayArtist(track) })}
              >
                {displayArtist(track)}
              </button>
              <div className="mt-0.5">
                <FormatBadge />
              </div>
            </>
          ) : (
            <div className="text-[13px] text-faint">Nothing playing</div>
          )}
        </div>
      </div>

      <div className="flex min-w-0 flex-col items-center gap-1">
        <TransportControls />
        <Seekbar />
      </div>

      <div className="flex items-center justify-end gap-1">
        <IconButton label="Now playing (F)" active={nowPlaying} onClick={toggleNowPlaying}>
          <Maximize2 size={16} />
        </IconButton>
        <IconButton label="Lyrics (L)" active={lyricsOpen} onClick={() => setLyricsOpen(!lyricsOpen)}>
          <Mic2 size={16} />
        </IconButton>
        <SleepButton />
        <div className="relative">
          <IconButton label="Downloads" active={downloadsOpen} onClick={toggleDownloads}>
            <Download size={17} />
          </IconButton>
          {downloadsBadge > 0 && (
            <span className="pointer-events-none absolute -right-0.5 -top-0.5 flex h-4 min-w-4 items-center justify-center rounded-full bg-accent px-1 text-[10px] font-bold text-black">
              {downloadsBadge > 99 ? "99+" : downloadsBadge}
            </span>
          )}
        </div>
        <IconButton label="Queue (Q)" active={queueOpen || view.kind === "queue"} onClick={toggleQueue}>
          <ListMusic size={18} />
        </IconButton>
        <VolumeControl />
      </div>
    </div>
  );
}
