import { ChevronLeft, ChevronRight, Search, X } from "lucide-react";
import { forwardRef, useEffect, useState } from "react";
import { useUi } from "../store/ui";

/** Back/forward navigation + library search (debounced, live). */
export const NavSearch = forwardRef<HTMLInputElement>(function NavSearch(_, ref) {
  const view = useUi((s) => s.view);
  const setView = useUi((s) => s.setView);
  const canBack = useUi((s) => s.back.length > 0);
  const canFwd = useUi((s) => s.forward.length > 0);
  const [q, setQ] = useState(view.kind === "search" ? view.q : "");

  useEffect(() => {
    if (view.kind !== "search") setQ("");
  }, [view.kind]);

  useEffect(() => {
    const h = setTimeout(() => {
      const cur = useUi.getState().view;
      if (q.trim()) {
        // Replace while typing so history doesn't fill with partial queries.
        setView({ kind: "search", q }, cur.kind === "search");
      } else if (cur.kind === "search") {
        useUi.getState().goBack();
      }
    }, 140);
    return () => clearTimeout(h);
  }, [q]);

  return (
    <div className="flex items-center gap-1">
      <button
        aria-label="Back"
        title="Back (Alt+←)"
        disabled={!canBack}
        onClick={() => useUi.getState().goBack()}
        className="rounded-md p-1 text-muted hover:bg-hover hover:text-fg disabled:opacity-30"
      >
        <ChevronLeft size={17} />
      </button>
      <button
        aria-label="Forward"
        title="Forward (Alt+→)"
        disabled={!canFwd}
        onClick={() => useUi.getState().goForward()}
        className="rounded-md p-1 text-muted hover:bg-hover hover:text-fg disabled:opacity-30"
      >
        <ChevronRight size={17} />
      </button>
      <div className="relative ml-2 w-[min(380px,40vw)]">
        <Search size={14} className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-faint" />
        <input
          ref={ref}
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              setQ("");
              (e.target as HTMLInputElement).blur();
            }
          }}
          placeholder="Search library (Ctrl+F)"
          spellCheck={false}
          className="h-7 w-full rounded-md bg-panel pl-8 pr-7 text-[12.5px] text-fg outline-none ring-1 ring-line placeholder:text-faint focus:ring-accent"
        />
        {q && (
          <button
            aria-label="Clear search"
            onClick={() => setQ("")}
            className="absolute right-1.5 top-1/2 -translate-y-1/2 rounded p-0.5 text-faint hover:text-fg"
          >
            <X size={13} />
          </button>
        )}
      </div>
    </div>
  );
});
