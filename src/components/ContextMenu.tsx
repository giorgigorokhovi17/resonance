import { ChevronRight } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { create } from "zustand";

export interface MenuItem {
  label: string;
  icon?: ReactNode;
  onClick?: () => void;
  danger?: boolean;
  disabled?: boolean;
  separator?: boolean;
  submenu?: MenuItem[];
}

interface MenuState {
  open: { x: number; y: number; items: MenuItem[] } | null;
  show: (x: number, y: number, items: MenuItem[]) => void;
  close: () => void;
}

export const useMenu = create<MenuState>((set) => ({
  open: null,
  show: (x, y, items) => set({ open: { x, y, items } }),
  close: () => set({ open: null }),
}));

export function openMenu(e: React.MouseEvent, items: MenuItem[]) {
  e.preventDefault();
  e.stopPropagation();
  useMenu.getState().show(e.clientX, e.clientY, items);
}

export const SEP: MenuItem = { label: "", separator: true };

function MenuList({ items, x, y, onDone }: { items: MenuItem[]; x: number; y: number; onDone: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x, y });
  const [sub, setSub] = useState<{ idx: number; x: number; y: number } | null>(null);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    setPos({
      x: Math.max(4, Math.min(x, window.innerWidth - r.width - 6)),
      y: Math.max(4, Math.min(y, window.innerHeight - r.height - 6)),
    });
  }, [x, y]);

  return (
    <div
      ref={ref}
      role="menu"
      className="fixed z-[100] min-w-[200px] max-w-[320px] rounded-lg bg-panel py-1.5 text-[13px] shadow-2xl ring-1 ring-line animate-in"
      style={{ left: pos.x, top: pos.y }}
      onContextMenu={(e) => e.preventDefault()}
    >
      {items.map((it, i) =>
        it.separator ? (
          <div key={i} className="my-1 h-px bg-line" />
        ) : (
          <button
            key={i}
            role="menuitem"
            disabled={it.disabled}
            onMouseEnter={(e) => {
              if (it.submenu) {
                const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
                setSub({ idx: i, x: r.right - 4, y: r.top - 6 });
              } else setSub(null);
            }}
            onClick={() => {
              if (it.submenu) return;
              it.onClick?.();
              onDone();
            }}
            className={`flex w-full items-center gap-2.5 px-3 py-1.5 text-left transition-colors disabled:opacity-40 ${
              it.danger ? "text-[#ef5b5b] hover:bg-[#ef5b5b]/10" : "hover:bg-hover"
            }`}
          >
            <span className="flex w-4 shrink-0 justify-center text-muted">{it.icon}</span>
            <span className="min-w-0 flex-1 truncate">{it.label}</span>
            {it.submenu && <ChevronRight size={14} className="text-faint" />}
          </button>
        ),
      )}
      {sub && items[sub.idx]?.submenu && (
        <MenuList items={items[sub.idx].submenu!} x={sub.x} y={sub.y} onDone={onDone} />
      )}
    </div>
  );
}

export function ContextMenuHost() {
  const open = useMenu((s) => s.open);
  const close = useMenu((s) => s.close);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!(e.target as HTMLElement).closest("[role=menu]")) close();
    };
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("mousedown", onDown, true);
    window.addEventListener("keydown", onKey);
    window.addEventListener("blur", close);
    window.addEventListener("resize", close);
    window.addEventListener("wheel", close, { passive: true });
    return () => {
      window.removeEventListener("mousedown", onDown, true);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("blur", close);
      window.removeEventListener("resize", close);
      window.removeEventListener("wheel", close);
    };
  }, [open, close]);

  if (!open) return null;
  return <MenuList key={`${open.x},${open.y}`} items={open.items} x={open.x} y={open.y} onDone={close} />;
}
