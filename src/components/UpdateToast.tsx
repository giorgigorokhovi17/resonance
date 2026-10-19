import { Download, Loader2, X } from "lucide-react";
import { useUpdater } from "../store/updater";

/** "A new version is available" card, shown with the toasts. */
export function UpdateToast() {
  const { phase, update, progress, dismissed, install, dismiss } = useUpdater();
  const busy = phase === "downloading" || phase === "installing";
  if (!update || dismissed || !(phase === "available" || busy)) return null;
  return (
    <div className="pointer-events-auto rounded-lg bg-panel p-3 text-[12.5px] shadow-2xl ring-1 ring-accent/50 animate-in">
      <div className="flex items-start gap-3">
        <Download size={16} className="mt-px shrink-0 text-accent" />
        <div className="min-w-0 flex-1 leading-snug">
          <div className="font-medium">Resonance {update.version} is available</div>
          {update.body && <div className="mt-0.5 line-clamp-3 text-muted">{update.body}</div>}
        </div>
        {!busy && (
          <button aria-label="Later" onClick={dismiss} className="text-faint hover:text-fg">
            <X size={14} />
          </button>
        )}
      </div>
      <div className="mt-2.5 flex items-center gap-3 pl-7">
        {busy ? (
          <>
            <Loader2 size={14} className="animate-spin text-accent" />
            <span className="text-muted">
              {phase === "installing"
                ? "Installing… Resonance will restart."
                : `Downloading${progress !== null ? ` ${Math.round(progress * 100)}%` : "…"}`}
            </span>
          </>
        ) : (
          <>
            <button
              onClick={install}
              className="accent-gradient rounded-full px-3.5 py-1 text-[12px] font-semibold text-on-accent shadow transition-transform hover:scale-[1.03]"
            >
              Update and restart
            </button>
            <button onClick={dismiss} className="text-[12px] text-muted hover:text-fg">
              Later
            </button>
          </>
        )}
      </div>
    </div>
  );
}
