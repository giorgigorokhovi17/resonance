import { Loader2, RotateCcw } from "lucide-react";
import { api } from "../lib/api";
import { usePlayer } from "../store/player";
import { useSoulseek } from "../store/soulseek";
import { Toggle } from "./SettingsView";

/** Turning YouTube downloads on, and the state of the tools they need. */
export function YouTubePanel() {
  const yt = useSoulseek((s) => s.yt);
  const toast = usePlayer((s) => s.pushToast);
  const set = (on: boolean) =>
    api
      .ytSetEnabled(on)
      .then((s) => useSoulseek.setState({ yt: s }))
      .catch((e) => toast("error", String(e)));

  return (
    <div className="space-y-3 text-[12.5px]">
      <label className="flex items-center gap-2 text-muted">
        <Toggle checked={yt.enabled} onChange={set} label="YouTube downloads" />
        Get songs from YouTube when Soulseek doesn't have them
      </label>
      {yt.enabled && (
        <div className="flex items-center gap-2 rounded-lg bg-bg px-3 py-2 ring-1 ring-line">
          {yt.state === "installing" && (
            <>
              <Loader2 size={14} className="animate-spin text-accent" />
              <span className="text-muted">Getting yt-dlp and QuickJS (about 20 MB)…</span>
            </>
          )}
          {yt.state === "ready" && (
            <>
              <span className="h-2 w-2 rounded-full bg-ok" />
              <span className="text-muted">Ready{yt.version && ` · yt-dlp ${yt.version}`}</span>
            </>
          )}
          {(yt.state === "failed" || yt.state === "missing") && (
            <>
              <span className="min-w-0 flex-1 text-[#d64545]">{yt.error ?? "The tools aren't installed."}</span>
              <button className="flex shrink-0 items-center gap-1 text-faint hover:text-fg" onClick={() => api.ytInstall()}>
                <RotateCcw size={13} /> Try again
              </button>
            </>
          )}
        </div>
      )}
      <p className="text-[11.5px] leading-relaxed text-faint">
        YouTube audio is AAC at about 128 kbps, below most Soulseek files, so Soulseek is always tried first. Files go into a YouTube folder inside
        your download folder. Turning this on downloads yt-dlp and QuickJS from their GitHub releases; they're kept up to date. Downloading from
        YouTube may be against its terms of service; only get music you have the right to.
      </p>
    </div>
  );
}
