import { useState, type ReactNode } from "react";
import {
  CaretDownIcon,
  CaretRightIcon,
  GitDiffIcon,
  WarningCircleIcon,
} from "@phosphor-icons/react";
import Expander from "~/features/chat/response/components/Expander";
import { actionIcon, entryIcon } from "~/features/chat/response/components/ToolIcon";
import { type Entry, entryActive, workKey } from "~/features/chat/response/lib/model";
import {
  actionOf,
  detailsOf,
  editedFile,
  entryLabel,
  groupLabel,
} from "~/features/chat/response/lib/tools";

/**
 * A run of work between two texts: one row, or a summary row over several that opens to list them.
 * While its last call runs, the summary shows that call instead, so the row always says what is
 * happening now rather than how many things have happened.
 */
export default function WorkGroup({
  entries,
  live,
  onOpenFile,
}: {
  entries: readonly Entry[];
  live: boolean;
  onOpenFile?: (path: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [first] = entries;
  const last = entries.at(-1);
  if (entries.length === 1 && first !== undefined) {
    return (
      <div className="pb-2">
        <EntryRow entry={first} live={live} entering={live} onOpenFile={onOpenFile} />
      </div>
    );
  }
  const current = live ? [...entries].reverse().find(entryActive) : undefined;
  const tools = entries.map((entry) => entry.tool);
  const failed = tools.some((tool) => tool.status === "error");
  const head = tools[0];
  return (
    <div className="pb-2">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        className={ENTRY_CLASS}
      >
        <IconColumn>
          {current ? (
            entryIcon(current)
          ) : failed ? (
            <WarningCircleIcon size={15} className="text-danger" />
          ) : (
            head && actionIcon(actionOf(head))
          )}
        </IconColumn>
        <EntryText entering={live} running={current !== undefined}>
          {current ? entryLabel(current) : groupLabel(tools)}
        </EntryText>
      </button>
      {open && (
        <Expander>
          {entries.map((entry) => (
            <EntryRow
              key={workKey(entry)}
              entry={entry}
              live={live}
              entering={live && entry === last}
              onOpenFile={onOpenFile}
            />
          ))}
        </Expander>
      )}
    </div>
  );
}

/**
 * One call or block of reasoning. It opens to show the command and its output, or the whole block
 * of reasoning; an edit that touched one file opens that file instead.
 */
function EntryRow({
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
          <IconColumn>{entryIcon(entry)}</IconColumn>
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
        <IconColumn>{entryIcon(entry)}</IconColumn>
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
const ENTRY_CLASS =
  "group flex min-h-6 w-full min-w-0 cursor-pointer items-center gap-1.5 rounded-[var(--radius-sm)] border-none bg-transparent px-0.5 py-0.5 text-left text-[length:var(--fs-md)] text-fg-muted hover:text-fg";

/** The box a row opens into: a command and its output. */
const DETAILS_CLASS =
  "ms-7 mt-0.5 mb-1 max-h-64 overflow-auto rounded-[var(--radius-sm)] bg-editor px-2 py-1.5 text-[length:var(--fs-sm)] text-fg-muted selectable";

/** The icon column. A fixed width keeps every label on the same left edge. */
export function IconColumn({ children }: { children: ReactNode }) {
  return <span className="grid w-6 flex-none place-items-center opacity-70">{children}</span>;
}

/**
 * The text of a row that has an icon beside it, and the sign that its work is still running.
 *
 * Nothing here fades. A label that dims and brightens on a loop reads as text that cannot settle,
 * and beside an icon the icon looks like it is going with it. So a running row's words keep their
 * brightness and a grey band sweeps across them instead, with no spinner beside them. Only a row
 * that has just arrived fades its text in; the rows that persist for a whole turn do not.
 */
function EntryText({
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
