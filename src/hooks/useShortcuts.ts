import { useEffect } from "react";
import { api } from "../lib/api";
import { livePosition, usePlayer } from "../store/player";
import { useUi } from "../store/ui";

function isTyping(e: KeyboardEvent) {
  const t = e.target as HTMLElement | null;
  return !!t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable);
}

/** Global in-app keyboard shortcuts. Media keys are handled natively through SMTC. */
export function useShortcuts(focusSearch: () => void, toggleMini: () => void) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key.toLowerCase() === "f") {
        e.preventDefault();
        focusSearch();
        return;
      }
      if (e.ctrlKey && e.key.toLowerCase() === "m") {
        e.preventDefault();
        toggleMini();
        return;
      }
      if (e.altKey && e.key === "ArrowLeft") {
        e.preventDefault();
        useUi.getState().goBack();
        return;
      }
      if (e.altKey && e.key === "ArrowRight") {
        e.preventDefault();
        useUi.getState().goForward();
        return;
      }
      if (isTyping(e)) return;
      const st = usePlayer.getState();
      const status = st.status;
      switch (e.key) {
        case " ":
          e.preventDefault();
          api.togglePlay();
          break;
        case "ArrowRight":
        case "ArrowLeft": {
          e.preventDefault();
          const dir = e.key === "ArrowRight" ? 1 : -1;
          if (e.ctrlKey) {
            dir > 0 ? api.next() : api.previous();
          } else if (status?.track) {
            const step = e.shiftKey ? 30 : 5;
            const target = Math.max(0, Math.min(status.duration - 0.5, livePosition() + dir * step));
            st.setStatus({ ...status, position: target });
            api.seek(target);
          }
          break;
        }
        case "ArrowUp":
        case "ArrowDown": {
          e.preventDefault();
          const v = Math.min(1, Math.max(0, (status?.volume ?? 0.8) + (e.key === "ArrowUp" ? 0.05 : -0.05)));
          st.setLocalVolume(v);
          api.setVolume(v);
          break;
        }
        case "m":
        case "M":
          api.setMuted(!(status?.muted ?? false));
          break;
        case "s":
        case "S":
          api.setShuffle(!st.queue.shuffle);
          break;
        case "r":
        case "R": {
          const r = st.queue.repeat;
          api.setRepeat(r === "off" ? "all" : r === "all" ? "one" : "off");
          break;
        }
        case "f":
        case "F":
          useUi.getState().toggleNowPlaying();
          break;
        case "q":
        case "Q":
          useUi.getState().toggleQueue();
          break;
        case "l":
        case "L": {
          const ui = useUi.getState();
          ui.setLyricsOpen(!ui.lyricsOpen);
          break;
        }
        case "Escape":
          if (useUi.getState().lyricsOpen) useUi.getState().setLyricsOpen(false);
          else useUi.getState().setNowPlaying(false);
          break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [focusSearch, toggleMini]);
}
