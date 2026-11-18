import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Moon, Square, Sun, X, Copy } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { flushSync } from "react-dom";
import logo from "../assets/logo.png";
import { useIsDark } from "../hooks/useAppearance";
import { saveAppearance } from "../views/settings/Visual";

/**
 * Custom frameless title bar. Everything that isn't a control carries `data-tauri-drag-region`,
 * so the window can be dragged (and double-clicked to maximize) from any empty spot — whether
 * maximized or not.
 */
export function TitleBar({ children }: { children?: ReactNode }) {
  const win = getCurrentWindow();
  const [maximized, setMaximized] = useState(false);

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
        <ThemeSwitch />
        <WinButton label="Minimize" onClick={() => win.minimize()}>
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
  onClick: (e: React.MouseEvent<HTMLButtonElement>) => void;
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

/**
 * Switches between dark and light. The new look grows out of the button as a circle (a view
 * transition); without motion it just switches.
 */
function ThemeSwitch() {
  const dark = useIsDark();
  const label = dark ? "Light mode" : "Dark mode";
  const flip = (e: React.MouseEvent<HTMLButtonElement>) => {
    const apply = () => flushSync(() => void saveAppearance({ theme: dark ? "light" : "dark" }));
    const doc = document as Document & { startViewTransition?: (cb: () => void) => { ready: Promise<void> } };
    if (!doc.startViewTransition || matchMedia("(prefers-reduced-motion: reduce)").matches) {
      apply();
      return;
    }
    const r = e.currentTarget.getBoundingClientRect();
    const x = r.left + r.width / 2;
    const y = r.top + r.height / 2;
    const radius = Math.hypot(Math.max(x, innerWidth - x), Math.max(y, innerHeight - y));
    doc.startViewTransition(apply).ready.then(() => {
      document.documentElement.animate(
        { clipPath: [`circle(0px at ${x}px ${y}px)`, `circle(${radius}px at ${x}px ${y}px)`] },
        { duration: 550, easing: "cubic-bezier(0.65, 0, 0.35, 1)", pseudoElement: "::view-transition-new(root)" },
      );
    });
  };
  return (
    <WinButton label={label} onClick={flip}>
      <span key={dark ? "d" : "l"} className="animate-in">
        {dark ? <Sun size={15} strokeWidth={1.6} /> : <Moon size={15} strokeWidth={1.6} />}
      </span>
    </WinButton>
  );
}
