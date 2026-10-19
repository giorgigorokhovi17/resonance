import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowDown, ArrowUp, AudioLines, Play } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { fmtTime, isHiRes } from "../lib/format";
import type { Track } from "../lib/types";
import { compareText } from "../store/library";
import { usePlayer } from "../store/player";
import { openMenu, type MenuItem } from "./ContextMenu";
import { trackMenuItems } from "./trackMenu";

export type Column = "index" | "title" | "artist" | "album" | "year" | "genre" | "quality" | "duration" | "added";

const COL_DEF: Record<Column, { label: string; width: string; align?: "right" }> = {
  index: { label: "#", width: "48px", align: "right" },
  title: { label: "Title", width: "minmax(180px,2.2fr)" },
  artist: { label: "Artist", width: "minmax(120px,1.4fr)" },
  album: { label: "Album", width: "minmax(120px,1.4fr)" },
  year: { label: "Year", width: "60px" },
  genre: { label: "Genre", width: "minmax(80px,0.8fr)" },
  quality: { label: "Quality", width: "112px" },
  duration: { label: "Time", width: "64px", align: "right" },
  added: { label: "Added", width: "96px" },
};

type SortKey = Exclude<Column, "index" | "quality"> | "quality";

function sortValue(t: Track, k: SortKey): string | number {
  switch (k) {
    case "title":
      return t.title;
    case "artist":
      return t.artist;
    case "album":
      return t.album;
    case "year":
      return t.year ?? 0;
    case "genre":
      return t.genre;
    case "quality":
      return (t.bitDepth ?? 0) * 1e6 + (t.sampleRate ?? 0);
    case "duration":
      return t.duration;
    case "added":
      return t.addedAt;
  }
}

export function sortTracks(tracks: Track[], key: SortKey, dir: 1 | -1): Track[] {
  const out = [...tracks];
  out.sort((a, b) => {
    const va = sortValue(a, key);
    const vb = sortValue(b, key);
    let c = typeof va === "number" ? (va as number) - (vb as number) : compareText(va as string, vb as string);
    if (c === 0) {
      // Stable, album-aware secondary ordering.
      c =
        compareText(a.albumArtist || a.artist, b.albumArtist || b.artist) ||
        compareText(a.album, b.album) ||
        (a.discNo ?? 1) - (b.discNo ?? 1) ||
        (a.trackNo ?? 0) - (b.trackNo ?? 0);
    }
    return c * dir;
  });
  return out;
}

function Quality({ t }: { t: Track }) {
  const hi = isHiRes(t);
  const label = t.bitDepth && t.sampleRate ? `${t.bitDepth}/${+(t.sampleRate / 1000).toFixed(1)}` : t.bitrate ? `${t.bitrate}k` : "";
  return (
    <span className="flex items-center gap-1.5 truncate text-[11.5px]">
      <span className={`rounded px-1 py-px text-[10px] font-semibold ring-1 ${hi ? "text-accent ring-accent/40" : "text-faint ring-line"}`}>
        {t.codec || "?"}
      </span>
      <span className="text-muted">{label}</span>
    </span>
  );
}

/**
 * Virtualized, sortable track table. Rows are 36px; handles 20k+ rows smoothly.
 * `onPlay(sortedTracks, index)` is called on double-click / Enter.
 */
export function TrackTable({
  tracks,
  columns = ["index", "title", "artist", "album", "year", "quality", "duration"],
  sortable = true,
  defaultSort,
  numbering = "row",
  header,
  onPlay,
  extraMenu,
  emptyText = "No tracks",
}: {
  tracks: Track[];
  columns?: Column[];
  sortable?: boolean;
  defaultSort?: { key: SortKey; dir: 1 | -1 };
  numbering?: "row" | "track";
  header?: ReactNode;
  onPlay: (sorted: Track[], index: number) => void;
  extraMenu?: (t: Track, index: number) => MenuItem[];
  emptyText?: string;
}) {
  const parentRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const [sort, setSort] = useState<{ key: SortKey; dir: 1 | -1 } | null>(defaultSort ?? null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [anchor, setAnchor] = useState<number | null>(null);
  const [margin, setMargin] = useState(0);
  const headRef = useRef<HTMLDivElement>(null);
  const [stuck, setStuck] = useState(false);
  const currentPath = usePlayer((s) => s.status?.track?.path);
  const playing = usePlayer((s) => s.status?.state === "playing");

  const sorted = useMemo(() => (sort ? sortTracks(tracks, sort.key, sort.dir) : tracks), [tracks, sort]);
  const template = columns.map((c) => COL_DEF[c].width).join(" ");

  useEffect(() => {
    setSelected(new Set());
    setAnchor(null);
  }, [sorted]);

  // The list may sit below a header inside the same scroll container.
  useLayoutEffect(() => {
    const el = listRef.current;
    if (!el) return;
    const measure = () => setMargin(el.offsetTop);
    measure();
    const ro = new ResizeObserver(measure);
    if (el.parentElement) ro.observe(el.parentElement);
    return () => ro.disconnect();
  }, [sorted.length > 0]);

  const virt = useVirtualizer({
    count: sorted.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 36,
    overscan: 12,
    scrollMargin: margin,
  });

  const toggleSort = (c: Column) => {
    if (!sortable || c === "index") return;
    const key = c as SortKey;
    setSort((s) => (s?.key === key ? (s.dir === 1 ? { key, dir: -1 } : null) : { key, dir: 1 }));
  };

  const onRowClick = (e: React.MouseEvent, i: number) => {
    if (e.shiftKey && anchor !== null) {
      const [a, b] = anchor < i ? [anchor, i] : [i, anchor];
      setSelected(new Set(Array.from({ length: b - a + 1 }, (_, k) => a + k)));
    } else if (e.ctrlKey) {
      const n = new Set(selected);
      n.has(i) ? n.delete(i) : n.add(i);
      setSelected(n);
      setAnchor(i);
    } else {
      setSelected(new Set([i]));
      setAnchor(i);
    }
  };

  const onContext = (e: React.MouseEvent, i: number) => {
    let sel = selected;
    if (!sel.has(i)) {
      sel = new Set([i]);
      setSelected(sel);
      setAnchor(i);
    }
    const chosen = [...sel].sort((a, b) => a - b).map((k) => sorted[k]);
    openMenu(
      e,
      trackMenuItems(chosen, {
        playAll: chosen.length === 1 ? () => onPlay(sorted, i) : undefined,
        extra: chosen.length === 1 && extraMenu ? extraMenu(sorted[i], i) : undefined,
      }),
    );
  };

  return (
    <div
      ref={parentRef}
      className="h-full overflow-y-auto outline-none"
      tabIndex={0}
      onScroll={(e) => {
        const h = headRef.current;
        const isStuck = !!h && (e.currentTarget as HTMLDivElement).scrollTop > h.offsetTop - 1;
        if (isStuck !== stuck) setStuck(isStuck);
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter" && anchor !== null) onPlay(sorted, anchor);
        if (e.key === "a" && e.ctrlKey) {
          e.preventDefault();
          setSelected(new Set(sorted.map((_, i) => i)));
        }
      }}
    >
      {header}
      <div
        ref={headRef}
        className={`sticky top-0 z-10 px-6 transition-[background-color,box-shadow] duration-200 ${
          stuck ? "bg-elev/85 shadow-[0_6px_16px_-10px_rgba(0,0,0,0.6)] backdrop-blur-xl" : "bg-transparent"
        }`}
      >
        <div
          className="grid items-center gap-3 border-b border-line/70 py-2.5 text-[11.5px] font-medium uppercase tracking-wider text-muted"
          style={{ gridTemplateColumns: template }}
        >
          {columns.map((c) => (
            <button
              key={c}
              onClick={() => toggleSort(c)}
              className={`flex items-center gap-1 truncate ${COL_DEF[c].align === "right" ? "justify-end" : ""} ${
                sortable && c !== "index" ? "hover:text-fg" : "cursor-default"
              } ${sort?.key === c ? "text-fg" : ""}`}
            >
              {COL_DEF[c].label}
              {sort?.key === c && (sort.dir === 1 ? <ArrowUp size={12} /> : <ArrowDown size={12} />)}
            </button>
          ))}
        </div>
      </div>
      {sorted.length === 0 ? (
        <div className="px-6 py-10 text-center text-[13px] text-faint">{emptyText}</div>
      ) : (
        <div ref={listRef} className="relative px-6" style={{ height: virt.getTotalSize() }}>
          {virt.getVirtualItems().map((vi) => {
            const t = sorted[vi.index];
            const isCurrent = t.path === currentPath;
            const isSel = selected.has(vi.index);
            return (
              <div
                key={vi.key}
                className={`group absolute left-6 right-6 grid items-center gap-3 rounded-md px-0 text-[13.5px] transition-colors duration-150 ${
                  isSel ? "bg-accent/15" : "hover:bg-hover"
                }`}
                style={{ top: vi.start - margin, height: vi.size, gridTemplateColumns: template }}
                onClick={(e) => onRowClick(e, vi.index)}
                onDoubleClick={() => onPlay(sorted, vi.index)}
                onContextMenu={(e) => onContext(e, vi.index)}
              >
                {columns.map((c) => (
                  <Cell
                    key={c}
                    c={c}
                    t={t}
                    index={vi.index}
                    numbering={numbering}
                    isCurrent={isCurrent}
                    playing={playing}
                    onPlay={() => onPlay(sorted, vi.index)}
                  />
                ))}
              </div>
            );
          })}
        </div>
      )}
      <div className="h-6" />
    </div>
  );
}

function Cell({
  c,
  t,
  index,
  numbering,
  isCurrent,
  playing,
  onPlay,
}: {
  c: Column;
  t: Track;
  index: number;
  numbering: "row" | "track";
  isCurrent: boolean;
  playing: boolean;
  onPlay: () => void;
}) {
  switch (c) {
    case "index":
      return (
        <span className="tabular flex justify-end text-[12px] text-faint">
          {isCurrent ? (
            <AudioLines size={15} className={`text-accent ${playing ? "animate-pulse" : ""}`} />
          ) : (
            <>
              <span className="group-hover:hidden">{numbering === "track" ? t.trackNo ?? "" : index + 1}</span>
              <button aria-label="Play" className="hidden text-fg group-hover:block" onClick={onPlay}>
                <Play size={13} fill="currentColor" />
              </button>
            </>
          )}
        </span>
      );
    case "title":
      return <span className={`truncate ${isCurrent ? "font-medium text-accent" : ""}`}>{t.title}</span>;
    case "artist":
      return <span className="truncate text-muted">{t.artist || t.albumArtist}</span>;
    case "album":
      return <span className="truncate text-muted">{t.album}</span>;
    case "year":
      return <span className="tabular text-muted">{t.year ?? ""}</span>;
    case "genre":
      return <span className="truncate text-muted">{t.genre}</span>;
    case "quality":
      return <Quality t={t} />;
    case "duration":
      return <span className="tabular text-right text-muted">{fmtTime(t.duration)}</span>;
    case "added":
      return <span className="tabular text-muted">{t.addedAt ? new Date(t.addedAt * 1000).toLocaleDateString() : ""}</span>;
  }
}
