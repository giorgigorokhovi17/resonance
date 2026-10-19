import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import logo from "./assets/logo.png";
import "./index.css";

// Block the WebView's default context menu and reload shortcuts in production builds.
if (!import.meta.env.DEV) {
  window.addEventListener("contextmenu", (e) => {
    const t = e.target as HTMLElement;
    if (!t.closest("input, textarea")) e.preventDefault();
  });
}

// Dev-only handle for automated UI checks.
if (import.meta.env.DEV) {
  Promise.all([import("./store/library"), import("./store/player"), import("./store/ui")]).then(([l, p, u]) => {
    (window as unknown as Record<string, unknown>).__stores = { library: l.useLibrary, player: p.usePlayer, ui: u.useUi };
  });
}

// The UI depends on the Tauri runtime (audio engine, library, window APIs). Opened in a plain
// browser tab (e.g. the bare Vite server), show how to launch it instead of a blank page.
const inTauri = "__TAURI_INTERNALS__" in window;

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    {inTauri ? (
      <App />
    ) : (
      <div className="flex h-full flex-col items-center justify-center gap-3 px-8 text-center">
        <img src={logo} alt="" className="h-12 w-12" />
        <h1 className="font-display text-[22px] font-semibold">Undertone runs inside its desktop shell</h1>
        <p className="max-w-md text-[13px] leading-relaxed text-muted">
          This is the frontend dev server only. Start the full app (audio engine, library, window) with{" "}
          <code className="rounded bg-panel px-1.5 py-0.5 text-fg ring-1 ring-line">corepack pnpm tauri dev</code>.
        </p>
      </div>
    )}
  </React.StrictMode>,
);
