import { useState } from "react";
import {
  BrainIcon,
  CaretDownIcon,
  CaretRightIcon,
  GitDiffIcon,
  WarningCircleIcon,
} from "@phosphor-icons/react";
import Markdown from "~/features/chat/components/Markdown";
import {
  DETAILS_CLASS,
  Expander,
  ROW_CLASS,
  RowIcon,
  RowLabel,
} from "~/features/chat/turn/components/Row";
import { actionIcon, entryIcon } from "~/features/chat/turn/components/ToolIcon";
import { type Entry, entryActive, workKey } from "~/features/chat/turn/lib/model";
import {
  actionOf,
  detailsOf,
  editedFile,
  entryLabel,
  groupLabel,
} from "~/features/chat/turn/lib/tools";

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
  const tools = entries.flatMap((entry) => (entry.kind === "tool" ? [entry.tool] : []));
  const failed = tools.some((tool) => tool.status === "error");
  const head = tools[0];
  return (
    <div className="pb-2">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        className={ROW_CLASS}
      >
        <RowIcon>
          {current ? (
            entryIcon(current)
          ) : failed ? (
            <WarningCircleIcon size={15} className="text-danger" />
          ) : head ? (
            actionIcon(actionOf(head))
          ) : (
            <BrainIcon size={15} />
          )}
        </RowIcon>
        <RowLabel entering={live} running={current !== undefined}>
          {current ? entryLabel(current) : groupLabel(tools)}
        </RowLabel>
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
        <button type="button" title={file} onClick={() => onOpenFile(file)} className={ROW_CLASS}>
          <RowIcon>{entryIcon(entry)}</RowIcon>
          <RowLabel
            entering={entering}
            running={active}
            className="underline-offset-2 group-hover:underline"
          >
            {entryLabel(entry)}
          </RowLabel>
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
        className={ROW_CLASS}
      >
        <RowIcon>{entryIcon(entry)}</RowIcon>
        <RowLabel entering={entering} running={active}>
          {entryLabel(entry)}
        </RowLabel>
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

/** What a row opens to show: reasoning as Markdown, a call as the command and its output. */
function EntryDetails({ entry }: { entry: Entry }) {
  if (entry.kind === "thinking") {
    return (
      <div className={DETAILS_CLASS}>
        <Markdown text={entry.part.text} />
      </div>
    );
  }
  return (
    <pre
      className={`${DETAILS_CLASS} m-0 font-[family-name:var(--font-mono)] break-words whitespace-pre-wrap`}
    >
      {detailsOf(entry)}
    </pre>
  );
}
