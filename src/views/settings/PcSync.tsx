import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import { api } from "../../lib/api";
import type { PcSyncOffer, PcSyncSettings, PcSyncStatus, PcSyncSummary, Settings } from "../../lib/types";
import { usePlayer } from "../../store/player";
import { Section, Toggle } from "../SettingsView";

async function save(pcSync: PcSyncSettings) {
  const st = usePlayer.getState();
  if (st.settings) st.setSettings({ ...st.settings, pcSync });
  try {
    usePlayer.getState().setSettings(await api.updateSettings({ pcSync }));
  } catch (e) {
    usePlayer.getState().pushToast("error", `Could not save settings: ${e}`);
  }
}

const songs = (n: number) => `${n} ${n === 1 ? "song" : "songs"}`;

function result(s: PcSyncSummary) {
  const parts = [];
  if (s.fetched) parts.push(`${songs(s.fetched)} from your PC`);
  if (s.sent) parts.push(`${songs(s.sent)} sent to your PC`);
  if (s.removed) parts.push(`${songs(s.removed)} removed`);
  if (s.skipped) parts.push(`${songs(s.skipped)} skipped`);
  return parts.length ? `Done: ${parts.join(", ")}.` : "Already up to date.";
}

/** Settings → Sync with PC (the phone app). */
export function PcSyncSection({ settings }: { settings: Settings }) {
  const p = settings.pcSync;
  const [link, setLink] = useState(p.link);
  const [offer, setOffer] = useState<PcSyncOffer | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [status, setStatus] = useState<PcSyncStatus | null>(null);

  const connect = (text: string) => {
    setProblem(null);
    api
      .pcSyncOffer(text)
      .then(setOffer)
      .catch((e) => {
        setOffer(null);
        setProblem(String(e));
      });
  };

  useEffect(() => {
    api.pcSyncStatus().then(setStatus).catch(() => {});
    const un = listen<PcSyncStatus>("pcsync:status", (e) => setStatus(e.payload));
    if (p.link) connect(p.link);
    return () => {
      un.then((f) => f());
    };
  }, []);

  const saveLink = () => {
    const text = link.trim();
    if (text && text !== p.link) save({ ...p, link: text });
    if (text) connect(text);
  };
  const toggleList = (id: number, on: boolean) => save({ ...p, lists: on ? [...p.lists, id] : p.lists.filter((l) => l !== id) });
  const toggleFolder = (path: string, on: boolean) => save({ ...p, folders: on ? [...p.folders, path] : p.folders.filter((f) => f !== path) });
  const sync = () => {
    setProblem(null);
    api.pcSyncStart().catch((e) => setProblem(String(e)));
  };

  const running = !!status?.running;
  const nothingPicked = !p.lists.length && !p.folders.length && !p.send;

  return (
    <Section
      title="Sync with PC"
      description="Keep playlists and music folders from your PC on this phone, and copy songs that are only on this phone to your PC. Works on your home Wi-Fi while Undertone is open on the PC."
    >
      <div className="space-y-4">
        <label className="block text-[13px]">
          <span className="block">Link from your PC</span>
          <span className="block text-[11.5px] text-faint">
            On the PC, open Settings → Phone sync. Scan the code with this phone's camera, copy the address it opens, and paste it here.
          </span>
          <input
            value={link}
            onChange={(e) => setLink(e.target.value)}
            onBlur={saveLink}
            onKeyDown={(e) => e.key === "Enter" && saveLink()}
            placeholder="http://192.168.1.20:8787/?k=…"
            spellCheck={false}
            autoCapitalize="off"
            autoCorrect="off"
            className="mt-2 w-full rounded-md bg-bg px-2 py-2 font-mono text-[12px] ring-1 ring-line focus:outline-none focus:ring-accent"
          />
        </label>

        {offer && (
          <div className="space-y-2">
            <div className="text-[13px]">Playlists to keep on this phone</div>
            {offer.lists.map((l) => (
              <div key={l.id} className="flex items-center justify-between gap-4 text-[13px]">
                <span className="min-w-0 truncate">
                  {l.name} <span className="text-[11.5px] text-faint">({songs(l.count)})</span>
                </span>
                <Toggle label={l.name} checked={p.lists.includes(l.id)} onChange={(v) => toggleList(l.id, v)} />
              </div>
            ))}
          </div>
        )}

        {offer && offer.folders.length > 0 && (
          <div className="space-y-2">
            <div className="text-[13px]">Music folders to keep on this phone</div>
            <div className="max-h-72 space-y-2 overflow-y-auto">
              {offer.folders.map((f) => {
                const parts = f.path.split("/");
                return (
                  <div key={f.path} className="flex items-center justify-between gap-4 text-[13px]" style={{ paddingLeft: (parts.length - 1) * 12 }}>
                    <span className="min-w-0 truncate">
                      {parts[parts.length - 1]} <span className="text-[11.5px] text-faint">({songs(f.count)})</span>
                    </span>
                    <Toggle label={f.path} checked={p.folders.includes(f.path)} onChange={(v) => toggleFolder(f.path, v)} />
                  </div>
                );
              })}
            </div>
            <p className="text-[11.5px] text-faint">Songs keep their folders, with their artwork and lyrics.</p>
          </div>
        )}

        {offer && <p className="text-[11.5px] text-faint">Turning a playlist or folder off removes its copies from this phone. Your PC keeps them.</p>}

        {offer && (
          <div className="flex items-center justify-between gap-4">
            <span className="min-w-0">
              <span className="block text-[13px]">Send songs to my PC</span>
              <span className="block text-[11.5px] text-faint">
                {offer.receive
                  ? "Copies songs that are only on this phone. They stay here too."
                  : "Turn on Receive songs from my phone in the PC's Settings first."}
              </span>
            </span>
            <Toggle label="Send songs to my PC" checked={p.send} onChange={(v) => save({ ...p, send: v })} />
          </div>
        )}

        {problem && <p className="rounded-lg bg-warn/10 p-3 text-[12.5px] text-warn ring-1 ring-warn/30">{problem}</p>}
        {!running && status?.error && <p className="rounded-lg bg-warn/10 p-3 text-[12.5px] text-warn ring-1 ring-warn/30">{status.error}</p>}
        {!running && status?.summary && <p className="text-[12.5px] text-muted">{result(status.summary)}</p>}
        {running && (
          <p className="text-[12.5px] text-muted">
            {status?.step || "Starting"}
            {status?.total ? ` (${status.done + 1} of ${status.total})` : ""}
          </p>
        )}

        {offer &&
          (running ? (
            <button onClick={() => api.pcSyncCancel()} className="rounded-md px-3 py-2 text-[13px] text-muted ring-1 ring-line hover:bg-hover hover:text-fg">
              Stop
            </button>
          ) : (
            <button
              onClick={sync}
              disabled={nothingPicked}
              className="rounded-md bg-accent px-3 py-2 text-[13px] font-medium text-bg disabled:opacity-50"
            >
              Sync now
            </button>
          ))}
      </div>
    </Section>
  );
}
