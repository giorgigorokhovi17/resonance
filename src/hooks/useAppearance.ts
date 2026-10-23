import { useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { CORNER_SCALE, DEFAULT_APPEARANCE, resolveTheme } from "../lib/theme";
import { usePlayer } from "../store/player";

/** Whether the app is currently dark, following the theme setting and the Windows theme. */
export function useIsDark(): boolean {
  const theme = usePlayer((s) => s.settings?.theme ?? "dark");
  const [systemDark, setSystemDark] = useState(() => window.matchMedia("(prefers-color-scheme: dark)").matches);
  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const on = () => setSystemDark(mq.matches);
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  }, []);
  return theme === "dark" || (theme === "system" && systemDark);
}

/**
 * Applies the theme mode and Visual settings to the document: colour tokens, corner radius,
 * ambient strength, and interface zoom. `coverColor` is the playing track's art colour.
 */
export function useAppearance(coverColor: string | null) {
  const dark = useIsDark();
  const a = usePlayer((s) => s.settings?.appearance) ?? DEFAULT_APPEARANCE;

  useEffect(() => {
    const root = document.documentElement;
    root.classList.toggle("dark", dark);
    const { vars } = resolveTheme(a, dark, coverColor);
    for (const [k, v] of Object.entries(vars)) root.style.setProperty(k, v);
    root.style.setProperty("--r", String(CORNER_SCALE[a.corners] ?? 1));
    root.style.setProperty("--ambient", String(a.ambientIntensity));
    root.style.setProperty("--np-blur", `${a.backdropBlur}px`);
  }, [a, dark, coverColor]);

  const zoom = a.uiScale;
  useEffect(() => {
    getCurrentWebview()
      .setZoom(zoom)
      .catch((e) => console.error("setZoom failed", e));
  }, [zoom]);
}
