import { Heart, Play, Shuffle } from "lucide-react";
import { useMemo } from "react";
import { EmptyState, PrimaryButton, SecondaryButton } from "../components/EmptyState";
import { TrackTable } from "../components/TrackTable";
import { ViewHeader } from "../components/ViewHeader";
import { playShuffled, playTracks } from "../lib/actions";
import { fmtDuration } from "../lib/format";
import type { Track } from "../lib/types";
import { useLibrary } from "../store/library";
import { useLoved } from "../store/loved";

export function LovedView() {
  const entries = useLoved((s) => s.entries);
  const allTracks = useLibrary((s) => s.allTracks);

  const byLowerPath = useMemo(() => {
    const map = new Map<string, Track>();
    for (const t of allTracks) map.set(t.path.toLowerCase(), t);
    return map;
  }, [allTracks]);

  const tracks = useMemo(() => {
    const out: Track[] = [];
    for (const e of entries) {
      const t = byLowerPath.get(e.path.toLowerCase());
      if (t) out.push(t);
    }
    return out;
  }, [entries, byLowerPath]);

  const total = useMemo(() => tracks.reduce((s, t) => s + t.duration, 0), [tracks]);

  if (!tracks.length) {
    return (
      <EmptyState
        icon={<Heart size={28} strokeWidth={1.4} />}
        title="No loved songs yet"
        body="Tap the heart on any song to keep it here."
      />
    );
  }

  return (
    <TrackTable
      tracks={tracks}
      columns={["index", "title", "artist", "album", "year", "quality", "duration"]}
      header={
        <ViewHeader
          title="Loved"
          subtitle={
            <>
              <span className="tabular">{tracks.length.toLocaleString()}</span> {tracks.length === 1 ? "song" : "songs"} ·{" "}
              <span className="tabular">{fmtDuration(total)}</span>
            </>
          }
          actions={
            <>
              <PrimaryButton onClick={() => playTracks(tracks, 0)}>
                <Play size={15} fill="currentColor" strokeWidth={0} /> Play
              </PrimaryButton>
              <SecondaryButton onClick={() => playShuffled(tracks)}>
                <Shuffle size={15} /> Shuffle
              </SecondaryButton>
            </>
          }
        />
      }
      onPlay={(sorted, i) => playTracks(sorted, i)}
    />
  );
}
