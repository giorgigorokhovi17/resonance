import { useVirtualizer } from "@tanstack/react-virtual";
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";

/**
 * Virtualized list with drag-to-reorder. Works with very large lists (only visible rows are
 * mounted) because the drop position is computed from the pointer, not from DOM targets.
 * Drag starts after the pointer moves 5px, so clicks and double-clicks still work.
 */
export function ReorderList<T>({
  items,
  getKey,
  rowHeight = 52,
  renderRow,
  onMove,
  header,
  scrollToIndex,
  className = "",
}: {
  items: T[];
  getKey: (item: T) => string | number;
  rowHeight?: number;
  renderRow: (item: T, index: number, dragging: boolean) => ReactNode;
  onMove: (from: number, to: number) => void;
  header?: ReactNode;
  scrollToIndex?: number;
  className?: string;
}) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const [margin, setMargin] = useState(0);
  const [drag, setDragState] = useState<{ from: number; insert: number; y: number } | null>(null);
  const dragRef = useRef<{ from: number; insert: number; y: number } | null>(null);
  const setDrag = (d: { from: number; insert: number; y: number } | null) => {
    dragRef.current = d;
    setDragState(d);
  };
  const pending = useRef<{ from: number; startY: number; pointerId: number } | null>(null);
  const autoScroll = useRef(0);

  useLayoutEffect(() => {
    const el = listRef.current;
    if (!el) return;
    const measure = () => setMargin(el.offsetTop);
    measure();
    const ro = new ResizeObserver(measure);
    if (el.parentElement) ro.observe(el.parentElement);
    return () => ro.disconnect();
  }, []);

  const virt = useVirtualizer({
    count: items.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => rowHeight,
    overscan: 10,
    scrollMargin: margin,
    getItemKey: (i) => getKey(items[i]),
  });

  useEffect(() => {
    if (scrollToIndex !== undefined && scrollToIndex >= 0 && scrollToIndex < items.length) {
      virt.scrollToIndex(scrollToIndex, { align: "center" });
    }
    // Only when the target changes, not on every list update.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scrollToIndex]);

  const insertAt = (clientY: number) => {
    const list = listRef.current!.getBoundingClientRect();
    const y = clientY - list.top;
    return Math.max(0, Math.min(items.length, Math.floor((y + rowHeight / 2) / rowHeight)));
  };

  useEffect(() => {
    const onMoveEv = (e: PointerEvent) => {
      const p = pending.current;
      if (!p) return;
      if (!dragRef.current && Math.abs(e.clientY - p.startY) < 5) return;
      const ins = insertAt(e.clientY);
      const sc = scrollRef.current!.getBoundingClientRect();
      autoScroll.current = e.clientY < sc.top + 48 ? -12 : e.clientY > sc.bottom - 48 ? 12 : 0;
      setDrag({ from: p.from, insert: ins, y: e.clientY });
    };
    const onUp = () => {
      const d = dragRef.current;
      pending.current = null;
      autoScroll.current = 0;
      setDrag(null);
      if (d) {
        const to = d.insert > d.from ? d.insert - 1 : d.insert;
        if (to !== d.from) onMove(d.from, to);
      }
    };
    window.addEventListener("pointermove", onMoveEv);
    window.addEventListener("pointerup", onUp);
    return () => {
      window.removeEventListener("pointermove", onMoveEv);
      window.removeEventListener("pointerup", onUp);
    };
  }, [items.length, onMove]);

  // Auto-scroll while dragging near the edges.
  useEffect(() => {
    if (!drag) return;
    let raf = 0;
    const tick = () => {
      const d = dragRef.current;
      if (autoScroll.current && scrollRef.current && d) {
        scrollRef.current.scrollTop += autoScroll.current;
        setDrag({ ...d, insert: insertAt(d.y) });
      }
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [!!drag]);

  return (
    <div ref={scrollRef} className={`h-full overflow-y-auto ${drag ? "cursor-grabbing select-none" : ""} ${className}`}>
      {header}
      <div ref={listRef} className="relative" style={{ height: virt.getTotalSize() }}>
        {virt.getVirtualItems().map((vi) => (
          <div
            key={vi.key}
            className={`absolute left-0 right-0 ${drag?.from === vi.index ? "opacity-40" : ""}`}
            style={{ top: vi.start - margin, height: vi.size }}
            onPointerDown={(e) => {
              if (e.button !== 0 || (e.target as HTMLElement).closest("button")) return;
              pending.current = { from: vi.index, startY: e.clientY, pointerId: e.pointerId };
            }}
          >
            {renderRow(items[vi.index], vi.index, drag?.from === vi.index)}
          </div>
        ))}
        {drag && (
          <div
            className="pointer-events-none absolute left-2 right-2 z-10 h-0.5 rounded-full bg-accent shadow-[0_0_8px_var(--c-accent)]"
            style={{ top: drag.insert * rowHeight - 1 }}
          />
        )}
      </div>
      <div className="h-6" />
    </div>
  );
}
