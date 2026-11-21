import { ChevronDown, Disc3, EllipsisVertical, ListMusic, Mic2 } from "lucide-react";
import { useMemo, useRef, useState } from "react";
import { Backdrop } from "../components/Backdrop";
import { openMenu } from "../components/ContextMenu";
import { Cover, coverUrl } from "../components/Cover";
import { FormatBadge } from "../components/FormatBadge";
import { EffectsButton, IconButton, SleepButton, TransportControls, VolumeControl } from "../components/PlayerBar";
import { Seekbar } from "../components/Seekbar";
import { trackMenuItems } from "../components/trackMenu";
import { api } from "../lib/api";
import { displayArtist, fmtTime } from "../lib/format";
import { isMobile } from "../lib/platform";
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

  const [vinyl, setVinyl] = useState(true);
  const swipeY = useRef<number | null>(null);
  const playing = status?.state === "playing";

  const upNext = useMemo(() => {
    const idx = queue.items.findIndex((i) => i.uid === queue.currentUid);
    return idx >= 0 ? queue.items.slice(idx + 1, idx + 6) : [];
  }, [queue]);

  if (!open) return null;
  const bg = coverUrl(coverId, "full");

  if (isMobile) {
    return (
      <div
        className={`fixed inset-0 z-40 overflow-hidden transition-colors duration-700 animate-np-sheet ${custom === "app" ? "" : "bg-bg"}`}
        style={{ backgroundColor: color && !custom ? `color-mix(in srgb, ${color} ${Math.round(38 * tint)}%, var(--c-bg))` : undefined }}
        role="dialog"
        aria-label="Now playing"
        onTouchStart={(e) => {
          swipeY.current = e.touches[0].clientY;
        }}
        onTouchEnd={(e) => {
          if (swipeY.current !== null && e.changedTouches[0].clientY - swipeY.current > 90) close();
          swipeY.current = null;
        }}
      >
        {custom === "nowPlaying" && skin && <Backdrop backdrop={skin} place="nowPlaying" scrim="bg" />}
        {bg && !custom && (
          <div className="absolute inset-0 transition-opacity duration-700" style={{ opacity: playing ? tint : tint * 0.72 }}>
            <div
              className="absolute -inset-24 scale-110 bg-cover bg-center opacity-55 transition-transform duration-1000"
              style={{
                backgroundImage: `url("${bg}")`,
                filter: "blur(var(--np-blur, 90px)) saturate(1.55)",
                transform: playing ? "scale(1.14)" : "scale(1.06)",
              }}
            />
          </div>
        )}
        {/* Solid at the edges, to meet the system bars above and below. */}
        {!custom && <div className="absolute inset-0 bg-gradient-to-b from-bg via-bg/50 to-bg" />}
        <div className="relative flex h-full flex-col px-6 pb-4 pt-[max(8px,env(safe-area-inset-top))]">
          <div className="flex h-12 shrink-0 items-center">
            <button onClick={close} aria-label="Close now playing" className="-ml-3 flex h-12 w-12 items-center justify-center transition-transform active:scale-90">
              <ChevronDown size={28} />
            </button>
            <div className="min-w-0 flex-1 truncate text-center text-[13px] font-medium text-fg/75">{track?.album || "Now playing"}</div>
            <button
              onClick={() => setVinyl((v) => !v)}
              aria-label={vinyl ? "Show album art" : "Show spinning vinyl"}
              title={vinyl ? "Switch to album art" : "Switch to spinning vinyl"}
              className={`flex h-11 w-11 items-center justify-center transition-transform active:scale-90 ${vinyl ? "text-accent" : "text-fg/70"}`}
            >
              <Disc3 size={21} className={vinyl && playing ? "vinyl-spin" : ""} />
            </button>
            <button
              disabled={!track}
              onClick={(e) => track && openMenu(e, trackMenuItems([track]))}
              aria-label="Options for this song"
              className="-mr-3 flex h-12 w-12 items-center justify-center transition-transform active:scale-90 disabled:opacity-30"
            >
              <EllipsisVertical size={22} />
            </button>
          </div>
          {!track ? (
            <div className="flex flex-1 flex-col items-center justify-center gap-3 text-muted">
              <Disc3 size={48} strokeWidth={1.2} />
              Nothing playing
            </div>
          ) : (
            <>
              {/* The cover takes what's left, so short screens keep the controls in view. */}
              <div className="flex min-h-0 flex-1 items-center justify-center py-3">
                {vinyl && !motionId ? (
                  <button
                    type="button"
                    onClick={() => setVinyl(false)}
                    aria-label="Switch to square album art"
                    className={`relative aspect-square max-h-full w-[min(78vw,310px)] rounded-full transition-transform duration-500 ${
                      playing ? "scale-100" : "scale-[0.91]"
                    }`}
                  >
                    <div
                      className={`vinyl-spin flex h-full w-full items-center justify-center rounded-full shadow-[0_24px_60px_-14px_rgba(0,0,0,0.82)] ring-1 ring-white/10 ${
                        playing ? "" : "paused"
                      }`}
                      style={{
                        background:
                          "repeating-radial-gradient(circle at center, #181820 0px, #101016 2.5px, #1d1d27 5px)",
                      }}
                    >
                      <Cover
                        coverId={coverId}
                        size="full"
                        className="h-[62%] w-[62%] shadow-[0_0_0_5px_rgba(0,0,0,0.45)]"
                        rounded="rounded-full"
                        iconSize={56}
                      />
                    </div>
                    <span className="pointer-events-none absolute left-1/2 top-1/2 h-4 w-4 -translate-x-1/2 -translate-y-1/2 rounded-full bg-bg ring-2 ring-white/25" />
                  </button>
                ) : (
                  <div
                    onClick={() => !motionId && setVinyl(true)}
                    className={`aspect-square max-h-full w-full max-w-[340px] transition-transform duration-500 ${
                      playing ? "scale-100" : "scale-[0.92]"
                    }`}
                  >
                    <Cover
                      coverId={coverId}
                      motionId={motionId}
                      motion="nowPlaying"
                      size="full"
                      className="h-full w-full shadow-[0_24px_60px_-20px_rgba(0,0,0,0.75)]"
                      rounded="rounded-2xl"
                      iconSize={96}
                    />
                  </div>
                )}
              </div>
              <div className="shrink-0">
                <h1 className="truncate font-display text-[24px] font-bold leading-tight">{track.title}</h1>
                <button
                  className="block max-w-full truncate text-left text-[16px] text-fg/75"
                  onClick={() => setView({ kind: "search", q: displayArtist(track) })}
                >
                  {displayArtist(track)}
                </button>
                <div className="mt-1.5">
                  <FormatBadge />
                </div>
                <div className="mt-2">
                  <Seekbar />
                </div>
                <div className="mt-1 flex justify-center">
                  <TransportControls big />
                </div>
                <div className="mt-3 flex items-center justify-around rounded-2xl bg-fg/5 ring-1 ring-fg/10">
                  <IconButton label="Lyrics" onClick={() => useUi.getState().setLyricsOpen(true)}>
                    <Mic2 size={20} />
                  </IconButton>
                  <EffectsButton />
                  <SleepButton />
                  <IconButton label="Queue" onClick={() => useUi.getState().toggleQueue()}>
                    <ListMusic size={21} />
                  </IconButton>
                </div>
              </div>
            </>
          )}
        </div>
      </div>
    );
  }

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
        // Centred, but never cut off at the top when the window is too short: then it scrolls.
        <div className="relative flex h-full items-center-safe justify-center gap-[5vw] overflow-y-auto px-[6vw] py-10">
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
