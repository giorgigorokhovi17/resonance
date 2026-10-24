import { listen } from "@tauri-apps/api/event";
import { Copy, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../../lib/api";
import type { PhoneStatus, PhoneSyncSettings, Settings } from "../../lib/types";
import { usePlayer } from "../../store/player";
import { Section, Toggle } from "../SettingsView";

async function save(phoneSync: PhoneSyncSettings) {
  const st = usePlayer.getState();
  if (st.settings) st.setSettings({ ...st.settings, phoneSync });
  try {
    usePlayer.getState().setSettings(await api.updateSettings({ phoneSync }));
  } catch (e) {
    usePlayer.getState().pushToast("error", `Could not save settings: ${e}`);
  }
}

/** Settings → General → Phone sync. */
export function PhoneSyncSection({ settings }: { settings: Settings }) {
  const p = settings.phoneSync;
  const [status, setStatus] = useState<PhoneStatus | null>(null);
  const [port, setPort] = useState(String(p.port));

  useEffect(() => {
    api.getPhoneSync().then(setStatus).catch(() => {});
    const un = listen<PhoneStatus>("phone:status", (e) => setStatus(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);
  useEffect(() => setPort(String(p.port)), [p.port]);

  const copy = () => {
    if (!status?.url) return;
    navigator.clipboard
      .writeText(status.url)
      .then(() => usePlayer.getState().pushToast("info", "Link copied."))
      .catch(() => {});
  };
  const savePort = () => {
    const n = Number(port);
    if (Number.isInteger(n) && n >= 1024 && n <= 65535 && n !== p.port) save({ ...p, port: n });
    else setPort(String(p.port));
  };

  return (
    <Section
      title="Phone sync"
      description="Play and download your playlists on your phone over your home Wi-Fi. Turn it on, then scan the code with your phone's camera (the phone must be on the same Wi-Fi). Downloads come as single songs or a whole playlist as a .zip with a playlist file your phone's music player can open."
    >
      <div className="space-y-4">
        <div className="flex items-center justify-between gap-4">
          <span className="min-w-0">
            <span className="block text-[13px]">Share my music on this Wi-Fi</span>
            <span className="block text-[11.5px] text-faint">
              Only devices with the secret link can see it, and only while Undertone runs. If Windows asks, allow access on private networks.
            </span>
          </span>
          <Toggle label="Phone sync" checked={p.enabled} onChange={(v) => save({ ...p, enabled: v })} />
        </div>

        {p.enabled && status?.error && <p className="rounded-lg bg-warn/10 p-3 text-[12.5px] text-warn ring-1 ring-warn/30">{status.error}</p>}

        {p.enabled && status?.running && status.url && (
          <div className="flex flex-wrap items-center gap-5 rounded-lg bg-bg p-4 ring-1 ring-line">
            {status.qr && (
              <div
                className="h-40 w-40 shrink-0 overflow-hidden rounded-lg bg-white p-1"
                aria-label="QR code for the phone link"
                // Made by the backend from the link alone (phone.rs).
                dangerouslySetInnerHTML={{ __html: status.qr }}
              />
            )}
            <div className="min-w-0 flex-1 space-y-3">
              <div className="text-[12.5px] text-muted">Scan with your phone's camera, or open this link in its browser:</div>
              <div className="break-all rounded-md bg-panel px-3 py-2 font-mono text-[11.5px] ring-1 ring-line">{status.url}</div>
              <div className="flex flex-wrap gap-2">
                <button onClick={copy} className="flex items-center gap-1.5 rounded-md px-2.5 py-1.5 text-[12px] text-muted ring-1 ring-line hover:bg-hover hover:text-fg">
                  <Copy size={13} /> Copy link
                </button>
                <button
                  onClick={() => api.newPhoneLink().then((s) => usePlayer.getState().setSettings(s)).catch(() => {})}
                  title="The old link stops working"
                  className="flex items-center gap-1.5 rounded-md px-2.5 py-1.5 text-[12px] text-muted ring-1 ring-line hover:bg-hover hover:text-fg"
                >
                  <RefreshCw size={13} /> New link
                </button>
              </div>
            </div>
          </div>
        )}

        {p.enabled && (
          <label className="flex items-center justify-between gap-4 text-[13px]">
            <span className="min-w-0">
              <span className="block">Port</span>
              <span className="block text-[11.5px] text-faint">Change it only if another app already uses {p.port}.</span>
            </span>
            <input
              type="number"
              min={1024}
              max={65535}
              value={port}
              onChange={(e) => setPort(e.target.value)}
              onBlur={savePort}
              onKeyDown={(e) => e.key === "Enter" && savePort()}
              className="w-24 rounded-md bg-bg px-2 py-1.5 text-right text-[12.5px] tabular ring-1 ring-line focus:outline-none focus:ring-accent"
            />
          </label>
        )}
      </div>
    </Section>
  );
}
