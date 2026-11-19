import { Music2, Shuffle } from "lucide-react";
import { useMemo, useState } from "react";
import { EmptyState, SecondaryButton } from "../components/EmptyState";
import { HiddenNote, MinLengthSelect } from "../components/MinLengthSelect";
import { ScanBanner } from "../components/ScanBanner";
import { TrackTable, type Column, type SortState } from "../components/TrackTable";
import { SortSelect, ViewHeader } from "../components/ViewHeader";
import { playShuffled, playTracks } from "../lib/actions";
import { fmtDuration } from "../lib/format";
import { useLibrary } from "../store/library";

type TrackOrder = "artist" | "recent" | "title" | "album" | "year" | "quality" | "duration";

const ORDER_TO_SORT: Record<TrackOrder, SortState> = {
  artist: { key: "artist", dir: 1 },
  recent: { key: "added", dir: -1 },
  title: { key: "title", dir: 1 },
  album: { key: "album", dir: 1 },
  year: { key: "year", dir: -1 },
  quality: { key: "quality", dir: -1 },
  duration: { key: "duration", dir: -1 },
};

const ORDER_OPTIONS: { value: TrackOrder; label: string }[] = [
  { value: "artist", label: "Artist" },
  { value: "recent", label: "Recently downloaded" },
  { value: "title", label: "Title" },
  { value: "album", label: "Album" },
  { value: "year", label: "Year" },
  { value: "quality", label: "Quality" },
  { value: "duration", label: "Duration" },
];

export function TracksView() {
  const tracks = useLibrary((s) => s.tracks);
  const allCount = useLibrary((s) => s.allTracks.length);
  const [sort, setSort] = useState<SortState | null>(ORDER_TO_SORT.artist);
  const total = useMemo(() => tracks.reduce((s, t) => s + t.duration, 0), [tracks]);
  if (!allCount) {
    return <EmptyState icon={<Music2 size={28} strokeWidth={1.4} />} title="No tracks yet" body="Add music folders in Settings." />;
  }
  const orderValue: TrackOrder =
    sort?.key === "added" ? "recent" : ((ORDER_OPTIONS.find((o) => ORDER_TO_SORT[o.value].key === sort?.key)?.value ?? "artist") as TrackOrder);
  const columns: Column[] =
    sort?.key === "added"
      ? ["index", "title", "artist", "album", "added", "quality", "duration"]
      : ["index", "title", "artist", "album", "year", "genre", "quality", "duration"];
  return (
    <TrackTable
      tracks={tracks}
      columns={columns}
      sort={sort}
      onSortChange={setSort}
      emptyText="Every track is shorter than the minimum length"
      header={
        <>
          <ScanBanner />
          <ViewHeader
            title="Tracks"
            subtitle={
              <>
                {tracks.length.toLocaleString()} tracks · {fmtDuration(total)}
                <HiddenNote count={allCount - tracks.length} />
              </>
            }
            actions={
              <>
                <SortSelect<TrackOrder>
                  label="Sort"
                  value={orderValue}
                  onChange={(v) => setSort(ORDER_TO_SORT[v])}
                  options={ORDER_OPTIONS}
                />
                <MinLengthSelect />
                <SecondaryButton onClick={() => playShuffled(tracks)}>
                  <Shuffle size={15} /> Shuffle all
                </SecondaryButton>
              </>
            }
          />
        </>
      }
      onPlay={(sorted, i) => playTracks(sorted, i)}
    />
  );
}
