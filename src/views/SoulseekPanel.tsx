import { open } from "@tauri-apps/plugin-dialog";
import { Download, FolderOpen, Loader2, LogOut, RotateCcw, Search, Square, X } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import { api } from "../lib/api";
import { fmtRate, fmtTime } from "../lib/format";
import type { SlskFile, SlskFolder, SlskJob, SlskSettingsView, SlskUser } from "../lib/types";
import { usePlayer } from "../store/player";
import { useSoulseek } from "../store/soulseek";
import { Toggle } from "./SettingsView";

const btn = "rounded-full px-4 py-1.5 text-[12.5px] font-medium text-muted ring-1 ring-line hover:bg-hover disabled:opacity-50";
const primary = "accent-gradient rounded-full px-4 py-1.5 text-[12.5px] font-semibold text-on-accent disabled:opacity-50";
const input = "rounded-lg bg-bg px-3 py-2 text-[13px] outline-none ring-1 ring-line placeholder:text-faint focus:ring-accent";

export function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 ? v.toFixed(0) : v.toFixed(1)} ${units[i]}`;
}

function quality(f: SlskFile): string {
  if (f.bitDepth && f.sampleRate) return `${f.bitDepth}-bit ${fmtRate(f.sampleRate)}`;
  if (f.sampleRate && !f.bitrate) return fmtRate(f.sampleRate);
  if (f.bitrate) return `${f.bitrate} kbps${f.vbr ? " VBR" : ""}`;
  return "";
}

/** Last two parts of a remote folder path; the rest is mostly share roots. */
function shortFolder(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts.slice(-2).join(" / ") || "(top level)";
}

export function SoulseekPanel() {
  const status = useSoulseek((s) => s.status);
  const jobs = useSoulseek((s) => s.jobs);
  const [otherAccount, setOtherAccount] = useState(false);
  if (status.state === "signedIn") return <SignedIn />;
  const saved = status.remembered && !!status.username && !otherAccount;
  return (
    <div className="space-y-4">
      {saved ? <Reconnect onOther={() => setOtherAccount(true)} /> : <SignIn />}
      {/* YouTube downloads share the queue, so it shows while signed out too. */}
      {jobs.length > 0 && <Queue jobs={jobs} />}
    </div>
  );
}

/** Signed out with a saved account: it signs in on its own; this is the button to do it now. */
function Reconnect({ onOther }: { onOther: () => void }) {
  const status = useSoulseek((s) => s.status);
  const connecting = status.state === "connecting";
  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center gap-3 text-[12.5px] text-muted">
        {connecting ? <Loader2 size={14} className="animate-spin text-accent" /> : <span className="h-2 w-2 rounded-full bg-faint" />}
        {connecting ? "Signing in as " : "Not connected. Saved account: "}
        <span className="font-medium text-fg">{status.username}</span>
        <button className="ml-auto text-faint hover:text-fg" onClick={onOther} disabled={connecting}>
          Use another account
        </button>
        <button className={primary} disabled={connecting} onClick={() => api.slskReconnect().catch(() => {})}>
          {connecting ? "Signing in…" : "Reconnect"}
        </button>
      </div>
      {status.error && !connecting && (
        <div className="text-[12.5px] text-[#d64545]">
          {status.error} {!status.error.includes("rejected") && "Undertone keeps trying in the background."}
        </div>
      )}
    </div>
  );
}

function SignIn() {
  const status = useSoulseek((s) => s.status);
  const [username, setUsername] = useState(status.username ?? "");
  const [password, setPassword] = useState("");
  const [remember, setRemember] = useState(true);
  const connecting = status.state === "connecting";

  useEffect(() => {
    if (!username && status.username) setUsername(status.username);
  }, [status.username]);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      await api.slskSignIn(username, password, remember);
      setPassword("");
    } catch {
      // The error shows under the form (it comes with the status event).
    }
  };

  return (
    <form onSubmit={submit} className="space-y-3">
      <div className="grid gap-2 sm:grid-cols-2">
        <input className={input} placeholder="Soulseek username" autoComplete="username" value={username} onChange={(e) => setUsername(e.target.value)} disabled={connecting} />
        <input className={input} placeholder="Password" type="password" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} disabled={connecting} />
      </div>
      <div className="flex flex-wrap items-center gap-3">
        <label className="flex items-center gap-2 text-[12.5px] text-muted">
          <Toggle checked={remember} onChange={setRemember} label="Remember me" />
          Remember me (saved in Windows Credential Manager)
        </label>
        <button type="submit" className={`${primary} ml-auto`} disabled={connecting || !username.trim() || !password}>
          {connecting ? "Signing in…" : "Sign in"}
        </button>
      </div>
      {status.error && <div className="text-[12.5px] text-[#d64545]">{status.error}</div>}
      <p className="text-[11.5px] leading-relaxed text-faint">
        No account? Signing in with a new name creates one. Soulseek sends the password without encryption, so use one you don't use anywhere else.
      </p>
    </form>
  );
}

function SignedIn() {
  const status = useSoulseek((s) => s.status);
  const jobs = useSoulseek((s) => s.jobs);
  const search = useSoulseek((s) => s.search);
  const startSearch = useSoulseek((s) => s.startSearch);
  const [query, setQuery] = useState(search?.query ?? "");

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (query.trim()) startSearch(query.trim());
  };

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-2 text-[12.5px] text-muted">
        <span className="h-2 w-2 rounded-full bg-ok" />
        Signed in as <span className="font-medium text-fg">{status.username}</span>
        <button className="ml-auto flex items-center gap-1.5 text-faint hover:text-fg" onClick={() => api.slskSignOut(false)}>
          <LogOut size={14} /> Sign out
        </button>
      </div>

      <form onSubmit={submit} className="flex gap-2">
        <div className="flex flex-1 items-center gap-2 rounded-lg bg-bg px-3 ring-1 ring-line focus-within:ring-accent">
          <Search size={15} className="text-faint" />
          <input
            className="min-w-0 flex-1 bg-transparent py-2 text-[13px] outline-none placeholder:text-faint"
            placeholder="Search Soulseek: artist, album or track"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            aria-label="Search Soulseek"
          />
        </div>
        <button type="submit" className={primary} disabled={query.trim().length < 3}>
          Search
        </button>
      </form>

      <FolderSettings />
      {jobs.length > 0 && <Queue jobs={jobs} />}
      {search && <Results />}
    </div>
  );
}

function FolderSettings() {
  const [view, setView] = useState<SlskSettingsView | null>(null);
  const [more, setMore] = useState(false);
  const toast = usePlayer((s) => s.pushToast);

  useEffect(() => {
    api.slskSettings().then(setView);
  }, []);
  if (!view) return null;

  const save = async (patch: Partial<SlskSettingsView>) => {
    const next = { ...view, ...patch };
    setView(await api.slskUpdateSettings({ downloadFolder: next.downloadFolder, listen: next.listen, listenPort: next.listenPort }));
  };
  const pick = async () => {
    const picked = await open({ directory: true, title: "Download folder" });
    if (typeof picked === "string") save({ downloadFolder: picked }).catch((e) => toast("error", String(e)));
  };

  return (
    <div className="rounded-lg bg-bg px-3 py-2.5 text-[12.5px] ring-1 ring-line">
      <div className="flex items-center gap-2">
        <FolderOpen size={15} className="shrink-0 text-accent" />
        <span className="text-muted">Downloads go to</span>
        <span className="min-w-0 flex-1 truncate" title={view.effectiveFolder ?? ""}>
          {view.effectiveFolder ?? "No folder yet: choose one, or add a library folder in Settings"}
        </span>
        <button className="shrink-0 text-faint hover:text-fg" onClick={pick}>
          Change
        </button>
        {view.downloadFolder && (
          <button className="shrink-0 text-faint hover:text-fg" onClick={() => save({ downloadFolder: null })}>
            Use default
          </button>
        )}
        <button className="shrink-0 text-faint hover:text-fg" onClick={() => setMore(!more)}>
          {more ? "Less" : "Connection"}
        </button>
      </div>
      {view.effectiveFolder && !view.inLibrary && (
        <div className="mt-1.5 text-[11.5px] text-warn">This folder isn't in your library, so downloads won't show up there until you add it.</div>
      )}
      {more && (
        <div className="mt-2.5 space-y-2 border-t border-line pt-2.5 text-muted">
          <label className="flex items-center gap-2">
            <Toggle checked={view.listen} onChange={(v) => save({ listen: v })} label="Accept incoming connections" />
            Accept incoming connections on port
            <input
              type="number"
              min={1024}
              max={65535}
              className="w-20 rounded bg-panel px-2 py-0.5 text-fg outline-none ring-1 ring-line focus:ring-accent"
              defaultValue={view.listenPort}
              onBlur={(e) => save({ listenPort: Number(e.target.value) || 2234 })}
            />
          </label>
          <p className="text-[11.5px] leading-relaxed text-faint">
            Other users connect to this port to send results and files. Behind a router without this port forwarded, most things still work, but
            users who are also behind one can't reach you. Changes apply the next time you sign in.
          </p>
        </div>
      )}
    </div>
  );
}

const stateLabel: Record<SlskJob["state"], string> = {
  queued: "Waiting",
  downloading: "Downloading",
  done: "Done",
  failed: "Failed",
  cancelled: "Cancelled",
};

function Queue({ jobs }: { jobs: SlskJob[] }) {
  const finished = jobs.some((j) => j.state === "done" || j.state === "failed" || j.state === "cancelled");
  return (
    <div>
      <div className="mb-1.5 flex items-center text-[12px] font-semibold uppercase tracking-wider text-faint">
        Downloads
        {finished && (
          <button className="ml-auto text-[12px] font-normal normal-case tracking-normal hover:text-fg" onClick={() => api.slskClearFinished()}>
            Clear finished
          </button>
        )}
      </div>
      <div className="max-h-72 overflow-y-auto rounded-lg ring-1 ring-line">
        {jobs.map((j) => (
          <JobRow key={j.id} job={j} />
        ))}
      </div>
    </div>
  );
}

function JobRow({ job }: { job: SlskJob }) {
  const pct = job.size > 0 ? Math.min(100, (job.bytes / job.size) * 100) : 0;
  const active = job.state === "queued" || job.state === "downloading";
  const tone = job.state === "done" ? "text-ok" : job.state === "failed" ? "text-[#d64545]" : "text-muted";
  return (
    <div className="border-b border-line px-3 py-2 last:border-b-0">
      <div className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <div className="truncate text-[13px]" title={job.remotePath}>
            {job.name}
          </div>
          <div className="truncate text-[11.5px] text-faint">
            from {job.username} · {fmtBytes(job.size)}
            {job.state === "downloading" && job.speed > 0 && ` · ${fmtBytes(job.speed)}/s`}
            {job.error && ` · ${job.error}`}
          </div>
        </div>
        <span className={`shrink-0 text-[12px] ${tone}`}>
          {job.state === "downloading" ? `${pct.toFixed(0)}%` : stateLabel[job.state]}
        </span>
        {active && (
          <button aria-label={`Cancel ${job.name}`} title="Cancel" className="text-faint hover:text-fg" onClick={() => api.slskCancel(job.id)}>
            <X size={15} />
          </button>
        )}
        {(job.state === "failed" || job.state === "cancelled") && (
          <button aria-label={`Retry ${job.name}`} title="Retry" className="text-faint hover:text-fg" onClick={() => api.slskRetry(job.id).catch(() => {})}>
            <RotateCcw size={14} />
          </button>
        )}
      </div>
      {active && (
        <div className="mt-1.5 h-1 overflow-hidden rounded-full bg-fg/10">
          <div className={`h-full rounded-full bg-accent transition-[width] ${job.state === "queued" ? "opacity-40" : ""}`} style={{ width: `${pct}%` }} />
        </div>
      )}
    </div>
  );
}

const PAGE = 25;

function Results() {
  const search = useSoulseek((s) => s.search)!;
  const [shown, setShown] = useState(PAGE);
  useEffect(() => setShown(PAGE), [search.id]);

  return (
    <div>
      <div className="mb-1.5 flex items-center gap-2 text-[12px] text-faint">
        {!search.done && <Loader2 size={13} className="animate-spin" />}
        <span>
          {search.fileCount.toLocaleString()} audio files from {search.users.length.toLocaleString()} users for “{search.query}”
          {!search.done && " · searching"}
        </span>
        {!search.done && (
          <button className="ml-auto flex items-center gap-1 hover:text-fg" onClick={() => api.slskStopSearch()}>
            <Square size={11} /> Stop
          </button>
        )}
      </div>
      {search.done && search.users.length === 0 && <div className="rounded-lg px-3 py-4 text-[12.5px] text-faint ring-1 ring-line">Nothing found.</div>}
      <div className="space-y-3">
        {search.users.slice(0, shown).map((u) => (
          <UserResults key={u.username} user={u} />
        ))}
      </div>
      {search.users.length > shown && (
        <button className={`${btn} mt-3`} onClick={() => setShown(shown + PAGE)}>
          Show more users
        </button>
      )}
    </div>
  );
}

function UserResults({ user }: { user: SlskUser }) {
  return (
    <div className="overflow-hidden rounded-lg ring-1 ring-line">
      <div className="flex items-center gap-2 bg-bg px-3 py-2 text-[12.5px]">
        <span className="font-semibold">{user.username}</span>
        <span className={user.freeSlots ? "text-ok" : "text-warn"}>{user.freeSlots ? "Free slot" : "Queue"}</span>
        {user.speed > 0 && <span className="text-faint">{fmtBytes(user.speed)}/s</span>}
      </div>
      {user.folders.map((f) => (
        <FolderResults key={f.path} user={user} folder={f} />
      ))}
    </div>
  );
}

function FolderResults({ user, folder }: { user: SlskUser; folder: SlskFolder }) {
  const toast = usePlayer((s) => s.pushToast);
  const get = async (files: SlskFile[]) => {
    try {
      const n = await api.slskDownload(files.map((f) => ({ username: user.username, path: f.path, size: f.size })));
      if (n === 0) toast("info", "Already in the download queue.");
    } catch (e) {
      toast("error", String(e));
    }
  };
  const total = folder.files.reduce((a, f) => a + f.size, 0);

  return (
    <div className="border-t border-line">
      <div className="flex items-center gap-2 px-3 pb-1 pt-2 text-[12px]">
        <FolderOpen size={13} className="shrink-0 text-faint" />
        <span className="min-w-0 flex-1 truncate text-muted" title={folder.path}>
          {shortFolder(folder.path)}
        </span>
        {folder.files.length > 1 && (
          <button className="flex shrink-0 items-center gap-1 text-faint hover:text-accent" onClick={() => get(folder.files)}>
            <Download size={13} /> All {folder.files.length} · {fmtBytes(total)}
          </button>
        )}
      </div>
      {folder.files.map((f) => (
        <div key={f.path} className="group flex items-center gap-3 px-3 py-1 text-[12.5px] hover:bg-hover">
          <span className="min-w-0 flex-1 truncate" title={f.name}>
            {f.name}
          </span>
          <span className="w-10 shrink-0 text-[11px] font-semibold text-muted">{f.format}</span>
          <span className="w-28 shrink-0 text-right text-faint">{quality(f)}</span>
          <span className="w-12 shrink-0 text-right tabular-nums text-faint">{f.length ? fmtTime(f.length) : ""}</span>
          <span className="w-16 shrink-0 text-right tabular-nums text-faint">{fmtBytes(f.size)}</span>
          <button aria-label={`Download ${f.name}`} title="Download" className="shrink-0 text-faint hover:text-accent" onClick={() => get([f])}>
            <Download size={14} />
          </button>
        </div>
      ))}
    </div>
  );
}
