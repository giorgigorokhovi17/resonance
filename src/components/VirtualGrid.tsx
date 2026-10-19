import { useVirtualizer } from "@tanstack/react-virtual";
import { useLayoutEffect, useRef, useState, type ReactNode } from "react";

/**
 * Virtualized responsive grid: only visible rows are rendered, so thousands of album covers
 * scroll smoothly. Items keep a square-ish card with a text footer of `footer` px.
 */
export function VirtualGrid<T>({
  items,
  minWidth = 176,
  footer = 52,
  gap = 8,
  header,
  render,
  getKey,
}: {
  items: T[];
  minWidth?: number;
  footer?: number;
  gap?: number;
  header?: ReactNode;
  render: (item: T) => ReactNode;
  getKey: (item: T) => string;
}) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const gridRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const [margin, setMargin] = useState(0);

  useLayoutEffect(() => {
    const el = gridRef.current;
    if (!el) return;
    const measure = () => {
      setWidth(el.clientWidth);
      setMargin(el.offsetTop);
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const cols = Math.max(1, Math.floor((width + gap) / (minWidth + gap)));
  const cardW = width > 0 ? (width - gap * (cols - 1)) / cols : minWidth;
  // Card = padding(16) + square art + footer text.
  const rowH = Math.round(cardW - 16 + footer + 16 + gap);
  const rows = Math.ceil(items.length / cols);

  const virt = useVirtualizer({
    count: rows,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => rowH,
    overscan: 3,
    scrollMargin: margin,
  });

  useLayoutEffect(() => {
    virt.measure();
  }, [rowH, cols]);

  return (
    <div ref={scrollRef} className="h-full overflow-y-auto">
      {header}
      <div ref={gridRef} className="relative mx-6" style={{ height: virt.getTotalSize() }}>
        {width > 0 &&
          virt.getVirtualItems().map((vr) => {
            const start = vr.index * cols;
            const slice = items.slice(start, start + cols);
            return (
              <div
                key={vr.key}
                className="absolute left-0 right-0 grid"
                style={{
                  top: vr.start - margin,
                  height: rowH,
                  gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))`,
                  columnGap: gap,
                }}
              >
                {slice.map((it) => (
                  <div key={getKey(it)}>{render(it)}</div>
                ))}
              </div>
            );
          })}
      </div>
      <div className="h-8" />
    </div>
  );
}
