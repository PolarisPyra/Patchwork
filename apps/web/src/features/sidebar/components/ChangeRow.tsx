import { type ReactNode } from "react";
import { GitDiffIcon } from "@phosphor-icons/react";
import { type FileChange } from "~/api";
import { baseName, dirName } from "~/lib/util";

/** A changed file's row, in the Changes view and in a reply's file grid. */

/** A small text button for acting on several changed files at once. */
export function BulkButton({ onClick, children }: { onClick: () => void; children: ReactNode }) {
  return (
    <button
      className="cursor-pointer rounded-[var(--radius-sm)] border-none bg-transparent px-1.5 py-0.5 text-[length:var(--fs-xs)] text-fg-muted hover:bg-hover hover:text-fg"
      onClick={onClick}
    >
      {children}
    </button>
  );
}

/** One changed file: opens its diff. */
/** `showDir` adds the file's folder, for rows not under a folder heading. */
export function ChangeRow({
  change,
  showDir = false,
  onOpen,
}: {
  change: FileChange;
  showDir?: boolean;
  onOpen: () => void;
}) {
  const name = baseName(change.path);
  if (showDir) {
    // A row of the reply's file grid, whose columns are shared so every folder starts together.
    return (
      <button className={REPLY_ROW_CLASS} title={change.path} onClick={onOpen}>
        <span
          className={`truncate ${change.status === "deleted" ? "text-fg-muted line-through" : ""}`}
        >
          {name}
        </span>
        <span className="truncate text-[length:var(--fs-xs)] text-fg-subtle">
          {dirName(change.path)}
        </span>
        <ChangeCounts change={change} />
      </button>
    );
  }
  return (
    <div className="flex h-[30px] items-center rounded-[var(--radius-sm)] py-1 pr-2 pl-1.5 hover:bg-hover">
      <button
        className="flex min-w-0 flex-1 cursor-pointer items-center gap-3 border-none bg-transparent p-0 text-fg text-left"
        title={change.path}
        onClick={onOpen}
      >
        <span
          className={`min-w-0 flex-1 truncate ${change.status === "deleted" ? "text-fg-muted line-through" : ""}`}
        >
          {name}
        </span>
        <ChangeCounts change={change} />
      </button>
    </div>
  );
}

/** A file's added and removed lines, then the changed icon. Each count sits at the right of a
 * fixed-width column, so the counts end against the icon and line up across rows, as the icon does. */
export function ChangeCounts({ change }: { change: FileChange }) {
  return (
    <span className="flex flex-none items-center justify-self-end text-[length:var(--fs-xs)] tabular-nums">
      <span className="flex items-center gap-3">
        <span className="w-[var(--diff-col)] text-right text-success">+{change.additions}</span>
        <span className="w-[var(--diff-col)] text-right text-danger">−{change.deletions}</span>
      </span>
      <span className="flex w-7 justify-center">
        <GitDiffIcon size={14} className="text-fg-muted" role="img" aria-label="Changed" />
      </span>
    </span>
  );
}

/** A row of the reply's file grid: name, folder, then counts or a note. */
export const REPLY_ROW_CLASS =
  "col-span-full grid h-[30px] cursor-pointer grid-cols-subgrid items-baseline rounded-[var(--radius-sm)] border-none bg-transparent py-1 pr-2 pl-1.5 text-left text-fg hover:bg-hover";
