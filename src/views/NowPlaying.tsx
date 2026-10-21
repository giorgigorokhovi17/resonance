import { ChevronDown, Disc3 } from "lucide-react";
import { useMemo } from "react";
import { Backdrop } from "../components/Backdrop";
import { Cover, coverUrl } from "../components/Cover";
import { FormatBadge } from "../components/FormatBadge";
import { TransportControls, VolumeControl } from "../components/PlayerBar";
import { Seekbar } from "../components/Seekbar";
import { api } from "../lib/api";
import { displayArtist, fmtTime } from "../lib/format";
import { liveCoverId, useLiveCover, useLiveMotion } from "../store/library";
import { useMedia } from "../store/media";
import { useCoverColor } from "../hooks/useCoverColor";
import { usePlayer } from "../store/player";
import { useUi } from "../store/ui";

/**
 * Full-size now-playing view with album art (moving, for a GIF or video cover) over a blurred-art
 * backdrop, or over the custom background from Settings → Visual.
 */
export function NowPlaying() {
  const open = useUi((s) => s.nowPlaying);
  const close = () => useUi.getState().setNowPlaying(false);
  const setView = useUi((s) => s.setView);
  const status = usePlayer((s) => s.status);
  const queue = usePlayer((s) => s.queue);
  const track = status?.track ?? queue.items.find((i) => i.uid === queue.currentUid)?.track ?? null;
  const coverId = useLiveCover(track);
  const motionId = useLiveMotion(track);
  const color = useCoverColor(coverId);
  const tint = usePlayer((s) => s.settings?.appearance.ambientIntensity ?? 1);
  const skin = usePlayer((s) => s.settings?.appearance.background ?? null);
  const scope = usePlayer((s) => s.settings?.appearance.backgroundScope ?? "app");
  const skinReady = useMedia((s) => !!skin && s.byId.has(skin.mediaId));
  // "app": the app's background shows through; "nowPlaying": it's drawn here only.
  const custom = skin && skinReady ? scope : null;

  const upNext = useMemo(() => {
    const idx = queue.items.findIndex((i) => i.uid === queue.currentUid);
    return idx >= 0 ? queue.items.slice(idx + 1, idx + 6) : [];
  }, [queue]);

  if (!open) return null;
  const bg = coverUrl(coverId, "full");

  return (
    <div
      className={`absolute inset-0 z-40 overflow-hidden transition-colors duration-700 animate-in ${custom === "app" ? "" : "bg-bg"}`}
      style={{ backgroundColor: color && !custom ? `color-mix(in srgb, ${color} ${Math.round(38 * tint)}%, var(--c-bg))` : undefined }}
      role="dialog"
      aria-label="Now playing"
    >
      {custom === "nowPlaying" && skin && <Backdrop backdrop={skin} place="nowPlaying" scrim="bg" />}
      {/* Blurred album-art backdrop */}
      {bg && !custom && (
        <div className="absolute inset-0" style={{ opacity: tint }}>
          <div
            className="absolute -inset-24 scale-110 bg-cover bg-center opacity-70 dark:opacity-55"
            style={{ backgroundImage: `url("${bg}")`, filter: "blur(var(--np-blur, 90px)) saturate(1.5)" }}
          />
        </div>
      )}
      {!custom && <div className="absolute inset-0 bg-gradient-to-b from-bg/30 via-bg/55 to-bg/90" />}

      <button
        onClick={close}
        aria-label="Close now playing"
        title="Close (Esc / F)"
        className="absolute right-5 top-4 z-10 rounded-full bg-black/20 p-2 text-fg/80 backdrop-blur hover:bg-black/35 hover:text-fg"
      >
        <ChevronDown size={20} />
      </button>

      {!track ? (
        <div className="relative flex h-full flex-col items-center justify-center gap-3 text-muted">
          <Disc3 size={48} strokeWidth={1.2} />
          Nothing playing
        </div>
      ) : (
        <div className="relative flex h-full items-center justify-center gap-[5vw] px-[6vw] py-10">
          <Cover
            coverId={coverId}
            motionId={motionId}
            motion="nowPlaying"
            size="full"
            className="aspect-square h-auto w-[min(56vh,40vw)] shrink-0 shadow-[0_30px_80px_-20px_rgba(0,0,0,0.65)]"
            rounded="rounded-xl"
            iconSize={96}
          />
          <div className="flex min-w-0 max-w-[560px] flex-1 flex-col">
            <div className="text-[12px] font-semibold uppercase tracking-[0.18em] text-fg/60">Now playing</div>
            <h1 className="mt-2 line-clamp-2 font-display text-[clamp(26px,3.4vw,46px)] font-bold leading-[1.1]">{track.title}</h1>
            <button
              className="mt-2 self-start truncate text-left text-[clamp(15px,1.5vw,20px)] font-medium text-fg/85 hover:underline"
              onClick={() => setView({ kind: "search", q: displayArtist(track) })}
            >
              {displayArtist(track)}
            </button>
            {track.album && (
              <div className="mt-0.5 self-start truncate text-left text-[14px] text-fg/60">
                {track.album}
                {track.year ? ` · ${track.year}` : ""}
              </div>
            )}

            <div className="mt-8">
              <Seekbar />
            </div>
            <div className="mt-3 flex items-center justify-between">
              <TransportControls big />
              <VolumeControl width="w-32" />
            </div>

            <div className="mt-8 rounded-xl bg-black/15 p-4 ring-1 ring-white/10 backdrop-blur-md dark:bg-black/25">
              <FormatBadge detailed />
            </div>

            {upNext.length > 0 && (
              <div className="mt-6">
                <div className="mb-2 text-[11.5px] font-semibold uppercase tracking-wider text-fg/55">Up next</div>
                <div className="space-y-0.5">
                  {upNext.map((it) => (
                    <button
                      key={it.uid}
                      onDoubleClick={() => api.playQueueItem(it.uid)}
                      title="Double-click to play"
                      className="flex w-full items-center gap-3 rounded-md px-2 py-1.5 text-left hover:bg-white/10"
                    >
                      <Cover coverId={liveCoverId(it.track)} className="h-8 w-8 shrink-0" rounded="rounded" iconSize={14} />
                      <span className="min-w-0 flex-1 truncate text-[13px]">
                        {it.track.title} <span className="text-fg/55">· {displayArtist(it.track)}</span>
                      </span>
                      <span className="tabular text-[12px] text-fg/55">{fmtTime(it.track.duration)}</span>
                    </button>
                  ))}
                </div>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
