import { listen } from "@tauri-apps/api/event";
import { RotateCcw, X } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../../lib/api";
import type { HotkeyAction, HotkeyProblem, HotkeySettings, Settings } from "../../lib/types";
import { usePlayer } from "../../store/player";
import { Section, Toggle } from "../SettingsView";

const ACTIONS: { action: HotkeyAction; label: string }[] = [
  { action: "playPause", label: "Play / pause" },
  { action: "next", label: "Next song" },
  { action: "previous", label: "Previous song" },
  { action: "volumeUp", label: "Volume up" },
  { action: "volumeDown", label: "Volume down" },
  { action: "mute", label: "Mute" },
];

/** The same as the backend's defaults (settings.rs). */
const DEFAULTS: HotkeySettings = {
  enabled: true,
  playPause: "Ctrl+Alt+Space",
  next: "Ctrl+Alt+ArrowRight",
  previous: "Ctrl+Alt+ArrowLeft",
  volumeUp: "Ctrl+Alt+ArrowUp",
  volumeDown: "Ctrl+Alt+ArrowDown",
  mute: "Ctrl+Alt+KeyM",
};

const MODIFIERS = new Set(["Control", "Alt", "Shift", "Meta", "AltGraph", "OS"]);
/** Keys that can be a shortcut on their own; any other key needs Ctrl, Alt, Shift or Win. */
const ALONE = /^(F([1-9]|1\d|2[0-4])|Media\w+|Audio\w+|Pause|ScrollLock)$/;

const KEY_NAMES: Record<string, string> = {
  ArrowLeft: "←",
  ArrowRight: "→",
  ArrowUp: "↑",
  ArrowDown: "↓",
  Super: "Win",
  Equal: "=",
  Minus: "-",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Backslash: "\\",
  BracketLeft: "[",
  BracketRight: "]",
  Semicolon: ";",
  Quote: "'",
  Backquote: "`",
};

/** "Ctrl+Alt+ArrowRight" → "Ctrl + Alt + →". */
function pretty(keys: string) {
  return keys
    .split("+")
    .map((k) => KEY_NAMES[k] ?? k.replace(/^Key(.)$/, "$1").replace(/^Digit(.)$/, "$1").replace(/^Numpad/, "Num "))
    .join(" + ");
}

/** A key press as a shortcut ("Ctrl+Alt+KeyM"), or null when it isn't one yet. */
function shortcutFrom(e: KeyboardEvent): string | null {
  if (MODIFIERS.has(e.key)) return null;
  const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean);
  if (!mods.length && !ALONE.test(e.code)) return null;
  return [...mods, e.code].join("+");
}

// Applied locally first; only the reply to the latest change is taken back from the backend.
let writeSeq = 0;
async function save(hotkeys: HotkeySettings) {
  const st = usePlayer.getState();
  if (!st.settings) return;
  st.setSettings({ ...st.settings, hotkeys });
  const seq = ++writeSeq;
  try {
    const s = await api.updateSettings({ hotkeys });
    if (seq === writeSeq) usePlayer.getState().setSettings(s);
  } catch (e) {
    usePlayer.getState().pushToast("error", `Could not save settings: ${e}`);
  }
}

/** Settings → General → Global shortcuts. */
export function HotkeysSection({ settings }: { settings: Settings }) {
  const h = settings.hotkeys;
  const [recording, setRecording] = useState<HotkeyAction | null>(null);
  const [problems, setProblems] = useState<HotkeyProblem[]>([]);

  useEffect(() => {
    api.getHotkeyProblems().then(setProblems).catch(() => {});
    const un = listen<HotkeyProblem[]>("hotkeys:problems", (e) => setProblems(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);

  // While recording, the shortcuts are off so Windows passes the keys to this page.
  useEffect(() => {
    if (!recording) return;
    api.pauseHotkeys(true).catch(() => {});
    let saved = false;
    const done = (next: HotkeySettings) => {
      saved = true;
      save(next);
      setRecording(null);
    };
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopImmediatePropagation();
      if (e.key === "Escape") {
        setRecording(null);
        return;
      }
      if ((e.key === "Backspace" || e.key === "Delete") && !e.ctrlKey && !e.altKey) {
        done({ ...h, [recording]: null });
        return;
      }
      const keys = shortcutFrom(e);
      if (!keys) return;
      // Taken by another action here: move it.
      const next: HotkeySettings = { ...h, [recording]: keys };
      for (const { action } of ACTIONS) if (action !== recording && next[action] === keys) next[action] = null;
      done(next);
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      // Saving registers the new set; without a save (Esc, leaving the page) bring the old back.
      if (!saved) api.pauseHotkeys(false).catch(() => {});
    };
  }, [recording]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <Section
      title="Global shortcuts"
      description="Control the music from any app, games included, without switching to Undertone. Click a shortcut, then press the keys you want (Esc cancels, Backspace clears it)."
    >
      <div className="space-y-4">
        <div className="flex items-center justify-between gap-4">
          <span className="text-[13px]">Use global shortcuts</span>
          <Toggle label="Use global shortcuts" checked={h.enabled} onChange={(v) => save({ ...h, enabled: v })} />
        </div>
        {h.enabled && (
          <>
            <div className="overflow-hidden rounded-lg ring-1 ring-line">
              {ACTIONS.map(({ action, label }) => {
                const keys = h[action];
                const problem = problems.find((p) => p.action === action && p.keys === keys);
                const listening = recording === action;
                return (
                  <div key={action} className="flex items-center gap-3 border-b border-line px-4 py-2.5 last:border-b-0">
                    <span className="min-w-0 flex-1">
                      <span className="block text-[13px]">{label}</span>
                      {problem && <span className="block text-[11.5px] text-warn">{problem.message}</span>}
                    </span>
                    <button
                      onClick={() => setRecording(listening ? null : action)}
                      className={`min-w-[9.5rem] rounded-md px-3 py-1.5 text-center text-[12px] font-medium tabular transition-colors ${
                        listening
                          ? "bg-accent text-on-accent"
                          : problem
                            ? "bg-bg text-warn ring-1 ring-warn/50 hover:bg-hover"
                            : "bg-bg text-fg ring-1 ring-line hover:bg-hover"
                      }`}
                    >
                      {listening ? "Press keys…" : keys ? pretty(keys) : <span className="text-faint">Not set</span>}
                    </button>
                    <button
                      aria-label={`Clear ${label}`}
                      title="Clear"
                      disabled={!keys}
                      onClick={() => save({ ...h, [action]: null })}
                      className="rounded-md p-1.5 text-faint transition-colors hover:bg-hover hover:text-fg disabled:invisible"
                    >
                      <X size={14} />
                    </button>
                  </div>
                );
              })}
            </div>
            <button
              onClick={() => save(DEFAULTS)}
              className="flex items-center gap-1.5 rounded-md px-2 py-1 text-[12px] text-muted hover:bg-hover hover:text-fg"
            >
              <RotateCcw size={13} /> Reset to defaults
            </button>
          </>
        )}
      </div>
    </Section>
  );
}
