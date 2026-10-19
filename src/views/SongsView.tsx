import { CheckCheck, Ellipsis, FolderPlus, Music2, Shuffle, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { openMenu, useMenu } from "../components/ContextMenu";
import { EmptyState, PrimaryButton, SecondaryButton } from "../components/EmptyState";
import { HiddenNote, MinLengthSelect } from "../components/MinLengthSelect";
import { ScanBanner } from "../components/ScanBanner";
import { SongCard } from "../components/SongCard";
import { trackMenuItems } from "../components/trackMenu";
import { SortSelect, ViewHeader } from "../components/ViewHeader";
import { VirtualGrid } from "../components/VirtualGrid";
import { playShuffled, playTracks } from "../lib/actions";
import type { Track } from "../lib/types";
import { compareText, useLibrary } from "../store/library";
import { usePlayer } from "../store/player";
import { useUi } from "../store/ui";

type SongSort = "artist" | "title" | "added" | "year";

function loadSort(): SongSort {
  try {
    return (localStorage.getItem("songs.sort") as SongSort) || "artist";
  } catch {
    return "artist";
  }
}

function isTyping(e: KeyboardEvent) {
  const t = e.target as HTMLElement | null;
  return !!t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT" || t.isContentEditable);
}

export function SongsView() {
  const tracks = useLibrary((s) => s.tracks);
  const allCount = useLibrary((s) => s.allTracks.length);
  const loaded = useLibrary((s) => s.loaded);
  const scanning = useLibrary((s) => s.scanning);
  const currentPath = usePlayer((s) => s.status?.track?.path);
  const playing = usePlayer((s) => s.status?.state === "playing");
  const [sort, setSortState] = useState<SongSort>(loadSort);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [anchor, setAnchor] = useState<number | null>(null);
  const setSort = (v: SongSort) => {
    setSortState(v);
    try {
      localStorage.setItem("songs.sort", v);
    } catch {
      /* ignore */
    }
  };

  const sorted = useMemo(() => {
    const a = [...tracks];
    const byArtist = (x: Track, y: Track) =>
      compareText(x.artist || x.albumArtist, y.artist || y.albumArtist) || compareText(x.title, y.title);
    switch (sort) {
      case "title":
        return a.sort((x, y) => compareText(x.title, y.title));
      case "added":
        return a.sort((x, y) => y.addedAt - x.addedAt || byArtist(x, y));
      case "year":
        return a.sort((x, y) => (y.year ?? 0) - (x.year ?? 0) || byArtist(x, y));
      default:
        return a.sort(byArtist);
    }
  }, [tracks, sort]);

  const indexOf = useMemo(() => new Map(sorted.map((t, i) => [t.id, i])), [sorted]);
  // Selected tracks in display order; ids that left the library drop out on their own.
  const chosen = useMemo(() => sorted.filter((t) => selected.has(t.id)), [sorted, selected]);
  const selecting = chosen.length > 0;
  const allSelected = selecting && chosen.length === sorted.length;

  const selectAll = () => setSelected(new Set(sorted.map((t) => t.id)));
  const clearSelection = () => {
    setSelected(new Set());
    setAnchor(null);
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (isTyping(e)) return;
      if (e.ctrlKey && e.key.toLowerCase() === "a") {
        e.preventDefault();
        selectAll();
      } else if (e.key === "Escape" && selecting && !useMenu.getState().open) {
        clearSelection();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const onSelect = (e: React.MouseEvent, t: Track) => {
    const i = indexOf.get(t.id)!;
    if (e.shiftKey && anchor !== null) {
      const [a, b] = anchor < i ? [anchor, i] : [i, anchor];
      const n = e.ctrlKey ? new Set(selected) : new Set<number>();
      for (let k = a; k <= b; k++) n.add(sorted[k].id);
      setSelected(n);
    } else {
      const n = new Set(selected);
      n.has(t.id) ? n.delete(t.id) : n.add(t.id);
      setSelected(n);
      setAnchor(i);
    }
  };

  const selectionMenu = () =>
    trackMenuItems(chosen, {
      playAll: chosen.length === 1 ? () => playTracks(sorted, indexOf.get(chosen[0].id)!) : undefined,
    });

  const onCardMenu = (e: React.MouseEvent, t: Track) => {
    if (selected.has(t.id)) openMenu(e, selectionMenu());
    else openMenu(e, trackMenuItems([t], { playAll: () => playTracks(sorted, indexOf.get(t.id)!) }));
  };

  if (loaded && allCount === 0 && !scanning) {
    return (
      <EmptyState
        icon={<Music2 size={28} strokeWidth={1.4} />}
        title="Your library is empty"
        body="Add the folders where your music lives. Resonance scans them, reads tags and artwork, and keeps watching for changes."
        action={
          <PrimaryButton onClick={() => useUi.getState().setView({ kind: "settings" })}>
            <FolderPlus size={16} /> Add music folder
          </PrimaryButton>
        }
      />
    );
  }

  return (
    <VirtualGrid<Track>
      items={sorted}
      minWidth={184}
      footer={58}
      getKey={(t) => String(t.id)}
      render={(t) => (
        <SongCard
          track={t}
          isCurrent={t.path === currentPath}
          playing={playing}
          onPlay={() => playTracks(sorted, indexOf.get(t.id)!)}
          selected={selected.has(t.id)}
          selecting={selecting}
          onSelect={(e) => onSelect(e, t)}
          onContextMenu={(e) => onCardMenu(e, t)}
        />
      )}
      header={
        <>
          <ScanBanner />
          <ViewHeader
            title="Songs"
            subtitle={
              selecting ? (
                `${chosen.length.toLocaleString()} of ${tracks.length.toLocaleString()} selected`
              ) : (
                <>
                  {tracks.length.toLocaleString()} songs
                  <HiddenNote count={allCount - tracks.length} />
                </>
              )
            }
            actions={
              selecting ? (
                <>
                  <SecondaryButton
                    onClick={(e) => {
                      const r = e.currentTarget.getBoundingClientRect();
                      useMenu.getState().show(r.left, r.bottom + 6, selectionMenu());
                    }}
                    title="Options for the selected songs"
                  >
                    <Ellipsis size={15} /> Options
                  </SecondaryButton>
                  {!allSelected && (
                    <SecondaryButton onClick={selectAll} title="Select all (Ctrl+A)">
                      <CheckCheck size={15} /> Select all
                    </SecondaryButton>
                  )}
                  <SecondaryButton onClick={clearSelection} title="Clear selection (Esc)">
                    <X size={15} /> Clear
                  </SecondaryButton>
                </>
              ) : (
                <>
                  <SortSelect<SongSort>
                    value={sort}
                    onChange={setSort}
                    options={[
                      { value: "artist", label: "Artist" },
                      { value: "title", label: "Title" },
                      { value: "added", label: "Recently added" },
                      { value: "year", label: "Year" },
                    ]}
                  />
                  <MinLengthSelect />
                  <SecondaryButton onClick={selectAll} title="Select all (Ctrl+A)">
                    <CheckCheck size={15} /> Select all
                  </SecondaryButton>
                  <SecondaryButton onClick={() => playShuffled(sorted)}>
                    <Shuffle size={15} /> Shuffle
                  </SecondaryButton>
                </>
              )
            }
          />
        </>
      }
    />
  );
}
