import { Music2, Shuffle } from "lucide-react";
import { EmptyState, SecondaryButton } from "../components/EmptyState";
import { HiddenNote, MinLengthSelect } from "../components/MinLengthSelect";
import { ScanBanner } from "../components/ScanBanner";
import { TrackTable } from "../components/TrackTable";
import { ViewHeader } from "../components/ViewHeader";
import { playShuffled, playTracks } from "../lib/actions";
import { fmtDuration } from "../lib/format";
import { useLibrary } from "../store/library";
import { useMemo } from "react";

export function TracksView() {
  const tracks = useLibrary((s) => s.tracks);
  const allCount = useLibrary((s) => s.allTracks.length);
  const total = useMemo(() => tracks.reduce((s, t) => s + t.duration, 0), [tracks]);
  if (!allCount) {
    return <EmptyState icon={<Music2 size={28} strokeWidth={1.4} />} title="No tracks yet" body="Add music folders in Settings." />;
  }
  return (
    <TrackTable
      tracks={tracks}
      columns={["index", "title", "artist", "album", "year", "genre", "quality", "duration"]}
      defaultSort={{ key: "artist", dir: 1 }}
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
