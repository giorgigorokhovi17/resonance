import { useEffect, useRef, useState } from "react";
import { create } from "zustand";

interface PromptState {
  open: null | {
    title: string;
    label?: string;
    initial: string;
    confirm: string;
    danger?: boolean;
    input: boolean;
    resolve: (v: string | null) => void;
  };
}

const usePrompt = create<PromptState>(() => ({ open: null }));

/** Promise-based text prompt (returns null when cancelled). */
export function promptText(title: string, initial = "", confirm = "Save", label?: string): Promise<string | null> {
  return new Promise((resolve) =>
    usePrompt.setState({ open: { title, initial, confirm, label, input: true, resolve } }),
  );
}

/** Promise-based confirmation dialog. */
export function confirmDialog(title: string, label: string, confirm = "Delete"): Promise<boolean> {
  return new Promise((resolve) =>
    usePrompt.setState({
      open: { title, label, initial: "", confirm, danger: true, input: false, resolve: (v) => resolve(v !== null) },
    }),
  );
}

export function DialogHost() {
  const open = usePrompt((s) => s.open);
  const [value, setValue] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setValue(open.initial);
      setTimeout(() => inputRef.current?.select(), 30);
    }
  }, [open]);

  if (!open) return null;
  const close = (v: string | null) => {
    open.resolve(v);
    usePrompt.setState({ open: null });
  };
  const submit = () => {
    if (open.input && !value.trim()) return;
    close(open.input ? value.trim() : "ok");
  };

  return (
    <div className="fixed inset-0 z-[120] flex items-center justify-center bg-black/50 backdrop-blur-sm animate-in" onMouseDown={() => close(null)}>
      <div
        role="dialog"
        aria-modal="true"
        aria-label={open.title}
        className="w-[420px] rounded-xl bg-panel p-5 shadow-2xl ring-1 ring-line"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") close(null);
          if (e.key === "Enter") submit();
          e.stopPropagation();
        }}
      >
        <h2 className="text-[15px] font-semibold">{open.title}</h2>
        {open.label && <p className="mt-1.5 text-[12.5px] leading-relaxed text-muted">{open.label}</p>}
        {open.input && (
          <input
            ref={inputRef}
            autoFocus
            value={value}
            onChange={(e) => setValue(e.target.value)}
            className="mt-4 h-9 w-full rounded-md bg-bg px-3 text-[13px] outline-none ring-1 ring-line focus:ring-accent"
          />
        )}
        <div className="mt-5 flex justify-end gap-2">
          <button onClick={() => close(null)} className="rounded-full px-4 py-1.5 text-[12.5px] font-medium text-muted ring-1 ring-line hover:bg-hover">
            Cancel
          </button>
          <button
            autoFocus={!open.input}
            onClick={submit}
            className={`rounded-full px-4 py-1.5 text-[12.5px] font-semibold text-white ${open.danger ? "bg-[#d64545] hover:bg-[#c43b3b]" : "accent-gradient"}`}
          >
            {open.confirm}
          </button>
        </div>
      </div>
    </div>
  );
}
