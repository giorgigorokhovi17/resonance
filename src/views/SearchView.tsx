import { Search } from "lucide-react";
import { useMemo } from "react";
import { EmptyState } from "../components/EmptyState";
import { TrackTable } from "../components/TrackTable";
import { playTracks } from "../lib/actions";
import { searchTracks, useLibrary } from "../store/library";

export function SearchView({ q }: { q: string }) {
  const tracksAll = useLibrary((s) => s.tracks);
  const tracks = useMemo(() => searchTracks(q, 2000), [q, tracksAll]);

  if (!q.trim()) {
    return <EmptyState icon={<Search size={28} strokeWidth={1.4} />} title="Search your library" body="Find songs by title, artist, album or genre. Press Ctrl+F anywhere." />;
  }
  if (!tracks.length) {
    return <EmptyState icon={<Search size={28} strokeWidth={1.4} />} title={`No results for “${q}”`} body="Try fewer or different words." />;
  }

  return (
    <TrackTable
      tracks={tracks}
      header={
        <h1 className="px-8 pb-4 pt-8 font-display text-[28px] font-bold">
          Results for <span className="accent-text">“{q}”</span>
          <span className="ml-3 align-middle text-[13px] font-normal text-muted">
            {tracks.length >= 2000 ? "2000+" : tracks.length} songs
          </span>
        </h1>
      }
      columns={["index", "title", "artist", "album", "quality", "duration"]}
      onPlay={(sorted, i) => playTracks(sorted, i)}
    />
  );
}
