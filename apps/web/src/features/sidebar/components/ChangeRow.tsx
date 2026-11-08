import { type ReactNode } from "react";

import { type FileChange } from "~/api";
import { baseName, dirName } from "~/lib/util";

/** Status letters, in the theme's colors. */
const STATUS: Record<string, { letter: string; label: string; color: string }> = {
  modified: { letter: "M", label: "Modified", color: "var(--warning)" },
  added: { letter: "A", label: "Added", color: "var(--success)" },
  deleted: { letter: "D", label: "Deleted", color: "var(--danger)" },
  renamed: { letter: "R", label: "Renamed", color: "var(--accent-text)" },
  untracked: { letter: "U", label: "Untracked", color: "var(--success)" },
  conflicted: { letter: "!", label: "Conflict", color: "var(--danger)" },
};

/** One file in Source Control: its name and folder, the actions shown on hover, its line
 * counts, and its status letter. Opens its diff. */
export function ChangeRow({
  change,
  onOpen,
  actions,
}: {
  change: FileChange;
  onOpen: () => void;
  /** Stage, discard, and the like, shown while the row is hovered or focused. */
  actions?: ReactNode;
}) {
  const status = STATUS[change.status] ?? STATUS.modified;
  const deleted = change.status === "deleted";
  return (
    <div className="group flex h-[22px] items-center gap-1 rounded-[var(--radius-sm)] pr-1 pl-5 hover:bg-hover">
      <button
        className="flex min-w-0 flex-1 cursor-pointer items-baseline gap-1.5 border-none bg-transparent p-0 text-left text-[length:var(--fs-sm)]"
        title={`${change.path} • ${status.label}`}
        onClick={onOpen}
      >
        <span
          className={`max-w-full flex-none truncate ${deleted ? "text-fg-muted line-through" : "text-fg"}`}
        >
          {baseName(change.path)}
        </span>
        <span className="min-w-0 truncate text-[length:var(--fs-xs)] text-fg-subtle">
          {dirName(change.path)}
        </span>
      </button>
      {actions}
      <span className="flex flex-none gap-1 text-right text-[length:var(--fs-xs)] tabular-nums">
        <span className="min-w-[4ch] text-success">+{change.additions}</span>
        <span className="min-w-[4ch] text-danger">−{change.deletions}</span>
      </span>
      <span
        className="w-3 flex-none text-center text-[length:var(--fs-xs)] font-semibold"
        style={{ color: status.color }}
        role="img"
        aria-label={status.label}
      >
        {status.letter}
      </span>
    </div>
  );
}
