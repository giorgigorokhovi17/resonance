import { Cloud, Download, ListPlus, MonitorPlay, Search } from "lucide-react";
import type { ReactNode } from "react";
import { ViewHeader } from "../components/ViewHeader";

/**
 * Placeholder for getting music in from elsewhere. Nothing here works yet: each card says
 * what is planned. Soulseek downloads go into a library folder; the playlist imports match
 * against files you already have.
 */
export function SourcesView() {
  return (
    <div className="h-full overflow-y-auto pb-8">
      <div className="mx-auto max-w-3xl">
        <ViewHeader title="Sources" subtitle="Get music and playlists in from other places. These are coming soon." />
        <div className="space-y-4 px-8">
          <SourceCard
            icon={<Download size={20} />}
            name="Soulseek"
            kind="Download"
            body="Search the Soulseek network and download files straight into a library folder. Sign in with your Soulseek account."
            planned={["Search by artist, album or track", "Download queue with progress", "Pick the download folder"]}
          >
            <div className="flex items-center gap-2 rounded-lg bg-bg px-3 py-2 text-[13px] text-faint ring-1 ring-line">
              <Search size={15} />
              Search Soulseek
            </div>
          </SourceCard>

          <SourceCard
            icon={<ListPlus size={20} />}
            name="Spotify"
            kind="Playlist import"
            body="Import playlists from an Exportify CSV or your Spotify data download. Songs you have are matched; the rest are listed as missing."
            planned={["Exportify CSV (most accurate)", "Spotify account data (matches go to review)"]}
          />

          <SourceCard
            icon={<MonitorPlay size={20} />}
            name="YouTube Music"
            kind="Playlist import"
            body="Import playlists from a Google Takeout export and match them to your files."
            planned={["Google Takeout playlists and library songs"]}
          />

          <SourceCard
            icon={<Cloud size={20} />}
            name="SoundCloud"
            kind="Playlist import"
            body="Import SoundCloud playlists and match them to your files."
            planned={["Not looked into yet"]}
          />
        </div>
      </div>
    </div>
  );
}

function SourceCard({
  icon,
  name,
  kind,
  body,
  planned,
  children,
}: {
  icon: ReactNode;
  name: string;
  kind: string;
  body: string;
  planned: string[];
  children?: ReactNode;
}) {
  return (
    <section className="rounded-xl bg-panel p-5 ring-1 ring-line" aria-disabled>
      <div className="flex items-start gap-4">
        <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-bg text-accent ring-1 ring-line">{icon}</div>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h2 className="text-[15px] font-semibold">{name}</h2>
            <span className="text-[12px] text-muted">{kind}</span>
            <span className="ml-auto rounded-full bg-bg px-2.5 py-0.5 text-[11px] font-semibold uppercase tracking-wider text-faint ring-1 ring-line">
              Coming soon
            </span>
          </div>
          <p className="mt-1 text-[12.5px] leading-relaxed text-muted">{body}</p>
          <ul className="mt-3 space-y-1 text-[12.5px] text-faint">
            {planned.map((p) => (
              <li key={p} className="flex items-center gap-2">
                <span className="h-1 w-1 rounded-full bg-faint" />
                {p}
              </li>
            ))}
          </ul>
          {children && <div className="pointer-events-none mt-4 select-none opacity-60">{children}</div>}
        </div>
      </div>
    </section>
  );
}
