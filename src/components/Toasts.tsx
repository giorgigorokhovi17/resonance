import { AlertTriangle, Info, X } from "lucide-react";
import { usePlayer } from "../store/player";
import { UpdateToast } from "./UpdateToast";

export function Toasts() {
  const toasts = usePlayer((s) => s.toasts);
  const dismiss = usePlayer((s) => s.dismissToast);
  return (
    <div className="pointer-events-none fixed bottom-28 right-5 z-50 flex w-[380px] flex-col gap-2" role="status" aria-live="polite">
      <UpdateToast />
      {toasts.map((t) => (
        <div
          key={t.id}
          className="pointer-events-auto flex items-start gap-3 rounded-lg bg-panel p-3 text-[12.5px] shadow-2xl ring-1 ring-line animate-in"
        >
          {t.kind === "error" ? (
            <AlertTriangle size={16} className="mt-px shrink-0 text-warn" />
          ) : (
            <Info size={16} className="mt-px shrink-0 text-accent" />
          )}
          <span className="min-w-0 flex-1 leading-snug">{t.message}</span>
          <button aria-label="Dismiss" onClick={() => dismiss(t.id)} className="text-faint hover:text-fg">
            <X size={14} />
          </button>
        </div>
      ))}
    </div>
  );
}
