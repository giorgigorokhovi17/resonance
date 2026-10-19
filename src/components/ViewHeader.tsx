import type { ReactNode } from "react";

export function ViewHeader({ title, subtitle, actions }: { title: string; subtitle?: ReactNode; actions?: ReactNode }) {
  return (
    <div className="flex flex-wrap items-end justify-between gap-4 px-8 pb-4 pt-6">
      <div className="min-w-0">
        <h1 className="font-display text-[28px] font-semibold leading-tight">{title}</h1>
        {subtitle && <div className="mt-1 text-[12.5px] text-muted">{subtitle}</div>}
      </div>
      {actions && <div className="flex items-center gap-2">{actions}</div>}
    </div>
  );
}

export function SortSelect<T extends string>({
  value,
  options,
  onChange,
  label = "Sort",
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
  label?: string;
}) {
  return (
    <label className="flex items-center gap-2 text-[12.5px] text-muted">
      {label}
      <select
        value={value}
        onChange={(e) => onChange(e.target.value as T)}
        className="rounded-md bg-panel px-2 py-1 text-[12.5px] text-fg outline-none ring-1 ring-line focus:ring-accent"
      >
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
    </label>
  );
}
