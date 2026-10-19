import type { ReactNode } from "react";

export function EmptyState({ icon, title, body, action }: { icon: ReactNode; title: string; body?: string; action?: ReactNode }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 px-8 text-center animate-in">
      <div className="mb-1 flex h-16 w-16 items-center justify-center rounded-2xl bg-panel text-faint ring-1 ring-line">{icon}</div>
      <h2 className="text-[17px] font-semibold">{title}</h2>
      {body && <p className="max-w-md text-[13px] leading-relaxed text-muted">{body}</p>}
      {action && <div className="mt-2">{action}</div>}
    </div>
  );
}

export function PrimaryButton({ children, onClick }: { children: ReactNode; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className="accent-gradient inline-flex items-center gap-2 rounded-full px-5 py-2 text-[13px] font-semibold text-on-accent shadow-md transition-transform hover:scale-[1.03] active:scale-[0.98]"
    >
      {children}
    </button>
  );
}

export function SecondaryButton({
  children,
  onClick,
  title,
}: {
  children: ReactNode;
  onClick: (e: React.MouseEvent<HTMLButtonElement>) => void;
  title?: string;
}) {
  return (
    <button
      onClick={onClick}
      title={title}
      className="inline-flex items-center gap-2 rounded-full px-4 py-2 text-[13px] font-medium text-fg ring-1 ring-line transition-colors hover:bg-hover"
    >
      {children}
    </button>
  );
}
