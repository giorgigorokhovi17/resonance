import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, X, Copy } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import logo from "../assets/logo.png";
import { toggleMiniPlayer } from "../lib/window";
import { usePlayer } from "../store/player";

/**
 * Custom frameless title bar. Everything that isn't a control carries `data-tauri-drag-region`,
 * so the window can be dragged (and double-clicked to maximize) from any empty spot — whether
 * maximized or not.
 */
export function TitleBar({ children }: { children?: ReactNode }) {
  const win = getCurrentWindow();
  const [maximized, setMaximized] = useState(false);
  const minimizeToMini = usePlayer((s) => s.settings?.minimizeToMini ?? false);

  useEffect(() => {
    let un: (() => void) | undefined;
    win.isMaximized().then(setMaximized);
    win.onResized(() => win.isMaximized().then(setMaximized)).then((f) => (un = f));
    return () => un?.();
  }, []);

  return (
    <div data-tauri-drag-region className="flex h-11 shrink-0 select-none items-center">
      <div data-tauri-drag-region className="flex h-full min-w-0 flex-1 items-center gap-2.5 pl-4">
        <img data-tauri-drag-region src={logo} alt="" draggable={false} className="h-[18px] w-[18px] shrink-0" />
        <span data-tauri-drag-region className="text-[12.5px] font-semibold tracking-wide text-muted">
          Undertone
        </span>
        <div data-tauri-drag-region className="flex h-full min-w-0 flex-1 items-center pl-6">
          {children}
          <div data-tauri-drag-region className="h-full flex-1" />
        </div>
      </div>
      <div className="flex h-full">
        <WinButton
          label={minimizeToMini ? "Mini player" : "Minimize"}
          onClick={() => (minimizeToMini ? toggleMiniPlayer() : win.minimize())}
        >
          <Minus size={15} strokeWidth={1.5} />
        </WinButton>
        <WinButton label={maximized ? "Restore" : "Maximize"} onClick={() => win.toggleMaximize()}>
          {maximized ? <Copy size={12} strokeWidth={1.5} className="-scale-x-100" /> : <Square size={12} strokeWidth={1.5} />}
        </WinButton>
        <WinButton label="Close" danger onClick={() => win.close()}>
          <X size={16} strokeWidth={1.5} />
        </WinButton>
      </div>
    </div>
  );
}

function WinButton({
  children,
  onClick,
  label,
  danger,
}: {
  children: ReactNode;
  onClick: () => void;
  label: string;
  danger?: boolean;
}) {
  return (
    <button
      aria-label={label}
      title={label}
      onClick={onClick}
      className={`flex h-full w-12 items-center justify-center text-muted transition-colors duration-150 ${
        danger ? "hover:bg-[#c42b1c] hover:text-white" : "hover:bg-hover hover:text-fg"
      }`}
    >
      {children}
    </button>
  );
}
