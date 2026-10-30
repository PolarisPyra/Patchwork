import { useState, type ReactNode } from "react";
import { CaretDownIcon, CaretRightIcon, GitDiffIcon } from "@phosphor-icons/react";
import Expander from "~/features/chat/response/components/Expander";
import { entryIcon } from "~/features/chat/response/components/ToolIcon";
import { type Entry, entryActive } from "~/features/chat/response/lib/model";
import { detailsOf, editedFile, entryLabel } from "~/features/chat/response/lib/tools";

/**
 * One call or block of reasoning. It opens to show the command and its output, or the whole block
 * of reasoning; an edit that touched one file opens that file instead.
 */
export default function AgentStepDisplay({
  entry,
  live,
  entering = false,
  onOpenFile,
}: {
  entry: Entry;
  live: boolean;
  /** The row has just arrived in a running reply, so its label settles into place. */
  entering?: boolean;
  onOpenFile?: (path: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const active = live && entryActive(entry);
  const file = onOpenFile ? editedFile(entry) : null;
  if (file !== null && onOpenFile) {
    return (
      <div className="min-w-0">
        <button type="button" title={file} onClick={() => onOpenFile(file)} className={ENTRY_CLASS}>
          <LineGutter>{entryIcon(entry)}</LineGutter>
          <EntryText
            entering={entering}
            running={active}
            className="underline-offset-2 group-hover:underline"
          >
            {entryLabel(entry)}
          </EntryText>
          <GitDiffIcon
            size={13}
            className="flex-none text-fg-subtle opacity-0 group-hover:opacity-100"
          />
        </button>
      </div>
    );
  }
  const Caret = open ? CaretDownIcon : CaretRightIcon;
  // An edit with a file opens the file; everything else opens its own details.
  return (
    <div className="min-w-0">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        className={ENTRY_CLASS}
      >
        <LineGutter>{entryIcon(entry)}</LineGutter>
        <EntryText entering={entering} running={active}>
          {entryLabel(entry)}
        </EntryText>
        <Caret size={12} className="flex-none text-fg-subtle opacity-0 group-hover:opacity-100" />
      </button>
      {open && (
        <Expander>
          <EntryDetails entry={entry} />
        </Expander>
      )}
    </div>
  );
}

/** What a row opens to show: the command and its output. */
function EntryDetails({ entry }: { entry: Entry }) {
  return (
    <pre
      className={`${DETAILS_CLASS} m-0 font-[family-name:var(--font-mono)] break-words whitespace-pre-wrap`}
    >
      {detailsOf(entry)}
    </pre>
  );
}

/** A row that can be opened: one line, full width, with room for an icon and a caret. */
export const ENTRY_CLASS =
  "group flex min-h-6 w-full min-w-0 cursor-pointer items-center gap-1.5 rounded-[var(--radius-sm)] border-none bg-transparent px-0 py-0.5 text-left text-[length:var(--fs-md)] text-fg-muted hover:text-fg";

/** The box a row opens into: a command and its output. */
const DETAILS_CLASS =
  "ms-7 mt-0.5 mb-1 max-h-64 overflow-auto rounded-[var(--radius-sm)] bg-editor px-2 py-1.5 text-[length:var(--fs-sm)] text-fg-muted selectable";

/** The line gutter. A fixed width keeps every label on the same left edge, and the icon sits flush
 * with the line's left edge, the same as the status line's words. */
export function LineGutter({ children }: { children: ReactNode }) {
  return (
    <span className="grid w-6 flex-none items-center justify-items-start opacity-70">
      {children}
    </span>
  );
}

/**
 * The text of a row that has an icon beside it, and the sign that its work is still running.
 *
 * Nothing here fades. A label that dims and brightens on a loop reads as text that cannot settle,
 * and beside an icon the icon looks like it is going with it. So a running row's words keep their
 * brightness and a grey band sweeps across them instead, with no spinner beside them. Only a row
 * that has just arrived fades its text in; the rows that persist for a whole turn do not.
 */
export function EntryText({
  entering = false,
  running = false,
  className = "",
  children,
}: {
  /** The row has just arrived, so its text fades in. */
  entering?: boolean;
  /** Its work is running, so a shimmer sweeps across its text. */
  running?: boolean;
  className?: string;
  children: ReactNode;
}) {
  return (
    <span
      className={`flex min-w-0 flex-1 items-center gap-1.5 ${entering ? "timeline-status-enter" : ""} ${className}`}
    >
      <span className={`min-w-0 truncate ${running ? "text-shimmer" : ""}`}>{children}</span>
    </span>
  );
}
