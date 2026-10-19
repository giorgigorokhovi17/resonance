import { ChevronRight, Folder, FolderTree, Play } from "lucide-react";
import { openMenu } from "../components/ContextMenu";
import { EmptyState } from "../components/EmptyState";
import { MinLengthSelect } from "../components/MinLengthSelect";
import { TrackTable } from "../components/TrackTable";
import { trackMenuItems } from "../components/trackMenu";
import { playTracks } from "../lib/actions";
import { useLibrary, type FolderNode } from "../store/library";
import { useUi } from "../store/ui";

/** All tracks under a folder node, in path order. */
function collect(node: FolderNode, folders: Map<string, FolderNode>): import("../lib/types").Track[] {
  const out = [...node.tracks];
  for (const c of node.children) {
    const n = folders.get(c);
    if (n) out.push(...collect(n, folders));
  }
  return out;
}

export function FoldersView({ path }: { path?: string }) {
  const folders = useLibrary((s) => s.folders);
  const roots = useLibrary((s) => s.roots);
  const setView = useUi((s) => s.setView);

  if (!roots.length) {
    return <EmptyState icon={<FolderTree size={28} strokeWidth={1.4} />} title="No library folders" body="Add music folders in Settings." />;
  }

  const node = path ? folders.get(path) : undefined;
  const childPaths = node ? node.children : roots;
  const children = childPaths.map((p) => folders.get(p)).filter(Boolean) as FolderNode[];

  // Breadcrumb from the containing root down to this folder.
  const crumbs: FolderNode[] = [];
  if (node) {
    const root = roots.find((r) => node.path.startsWith(r));
    let cur: string | undefined = node.path;
    while (cur) {
      const n = folders.get(cur);
      if (!n) break;
      crumbs.unshift(n);
      if (cur === root) break;
      const s = cur.includes("\\") ? "\\" : "/";
      const i = cur.lastIndexOf(s);
      cur = i > 0 ? cur.slice(0, i) : undefined;
    }
  }

  const header = (
    <div className="px-8 pb-3 pt-6">
      <div className="flex flex-wrap items-center gap-1 text-[13px] text-muted">
        <button className="hover:text-fg" onClick={() => setView({ kind: "folders" })}>
          Folders
        </button>
        {crumbs.map((c) => (
          <span key={c.path} className="flex items-center gap-1">
            <ChevronRight size={14} className="text-faint" />
            <button className="max-w-[260px] truncate hover:text-fg" title={c.path} onClick={() => setView({ kind: "folders", path: c.path })}>
              {c === crumbs[0] ? c.path : c.name}
            </button>
          </span>
        ))}
      </div>
      <div className="mt-2 flex flex-wrap items-center justify-between gap-3">
        <h1 className="min-w-0 truncate font-display text-[26px] font-semibold">{node ? node.name : "Library folders"}</h1>
        <MinLengthSelect />
      </div>
      {children.length > 0 && (
        <div className="mt-4 grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-2">
          {children.map((c) => (
            <div
              key={c.path}
              className="group flex cursor-pointer items-center gap-3 rounded-lg bg-panel px-3 py-2.5 ring-1 ring-line transition-colors hover:bg-hover"
              onClick={() => setView({ kind: "folders", path: c.path })}
              onContextMenu={(e) => openMenu(e, trackMenuItems(collect(c, folders)))}
              title={c.path}
            >
              <Folder size={18} className="shrink-0 text-accent" />
              <div className="min-w-0 flex-1">
                <div className="truncate text-[13px] font-medium">{node ? c.name : c.path}</div>
                <div className="text-[11.5px] text-faint">{c.total.toLocaleString()} tracks</div>
              </div>
              <button
                aria-label={`Play ${c.name}`}
                className="hidden h-7 w-7 items-center justify-center rounded-full bg-fg text-bg group-hover:flex"
                onClick={(e) => {
                  e.stopPropagation();
                  playTracks(collect(c, folders), 0);
                }}
              >
                <Play size={12} fill="currentColor" strokeWidth={0} />
              </button>
            </div>
          ))}
        </div>
      )}
      {node && node.tracks.length > 0 && <h2 className="mt-6 text-[14px] font-semibold">Files</h2>}
    </div>
  );

  if (!node || node.tracks.length === 0) {
    return <div className="h-full overflow-y-auto">{header}</div>;
  }
  return (
    <TrackTable
      tracks={node.tracks}
      header={header}
      sortable
      columns={["index", "title", "artist", "album", "quality", "duration"]}
      onPlay={(sorted, i) => playTracks(sorted, i)}
    />
  );
}
