import { type ReactNode } from "react";
import Spinner from "~/components/Spinner";

/**
 * The parts every row of a turn is built from: the clickable row itself, the icon column, the
 * label, and the box a row opens into. Kept in one module so a row's spacing and hover cannot
 * drift from the row beneath it.
 */

/** A row that can be opened: one line, full width, with room for an icon and a caret. */
export const ROW_CLASS =
  "group flex min-h-6 w-full min-w-0 cursor-pointer items-center gap-1.5 rounded-[var(--radius-sm)] border-none bg-transparent px-0.5 py-0.5 text-left text-[length:var(--fs-md)] text-fg-muted hover:text-fg";

/** The head line of a turn, which is a row that opens the folded work. */
export const FOLD_CLASS =
  "flex cursor-pointer items-center gap-1.5 rounded-[var(--radius-sm)] border-none bg-transparent px-1 text-[length:var(--fs-md)] text-fg-muted hover:text-fg disabled:cursor-default disabled:hover:text-fg-muted";

/** The box a row opens into: a command and its output, or a block of reasoning. */
export const DETAILS_CLASS =
  "ms-7 mt-0.5 mb-1 max-h-64 overflow-auto rounded-[var(--radius-sm)] bg-editor px-2 py-1.5 text-[length:var(--fs-sm)] text-fg-muted selectable";

/** The icon column. A fixed width keeps every label on the same left edge. */
export function RowIcon({ children }: { children: ReactNode }) {
  return <span className="grid w-6 flex-none place-items-center opacity-70">{children}</span>;
}

/**
 * The text of a row that has an icon beside it, and the sign that its work is still running.
 *
 * Nothing here fades. A label that dims and brightens on a loop reads as text that cannot settle,
 * and beside an icon the icon looks like it is going with it. So the text is painted once at full
 * strength and a small ring-and-arc spinner carries the running cue, which leaves the words alone.
 * Only a row that has just arrived fades its text in; the rows that persist for a whole turn do not.
 */
export function RowLabel({
  entering = false,
  running = false,
  className = "",
  children,
}: {
  /** The row has just arrived, so its text fades in. */
  entering?: boolean;
  /** Its work is running, so the row carries a spinner. */
  running?: boolean;
  className?: string;
  children: ReactNode;
}) {
  return (
    <span
      className={`flex min-w-0 flex-1 items-center gap-1.5 ${entering ? "timeline-status-enter" : ""} ${className}`}
    >
      <span className="min-w-0 truncate">{children}</span>
      {running && <Spinner size={11} className="text-fg-subtle" />}
    </span>
  );
}

/**
 * A part of a row that opens: it grows its row height from 0fr to 1fr, so the rows below slide
 * down with it instead of jumping. `overflow-hidden` lets the height be the only thing animating.
 */
export function Expander({ children }: { children: ReactNode }) {
  return (
    <div className="timeline-expand timeline-expand-enter overflow-hidden">
      <div className="flex min-w-0 flex-col">{children}</div>
    </div>
  );
}
