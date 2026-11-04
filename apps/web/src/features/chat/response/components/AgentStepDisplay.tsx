import { useState, type ReactNode } from "react";
import {
  CaretDownIcon,
  CaretRightIcon,
  CheckIcon,
  GitDiffIcon,
  XIcon,
} from "@phosphor-icons/react";
import { type ToolCall } from "~/api";
import Spinner from "~/components/Spinner";
import Markdown from "~/features/chat/markdown/Markdown";
import Expander from "~/features/chat/response/components/Expander";
import { entryIcon } from "~/features/chat/response/components/ToolIcon";
import { type Entry, entryActive } from "~/features/chat/response/model";
import {
  type CommandResult,
  commandResult,
  detailsOf,
  editedFile,
  entryLabel,
  linesOf,
} from "~/features/chat/response/tools";

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
  const tool = entry.kind === "tool" ? entry.tool : null;
  const status = {
    running: active,
    done: !active && tool?.status === "done",
    failed: tool?.status === "error",
    lines: tool && linesOf(tool),
    result: tool && commandResult(tool),
  };
  const file = onOpenFile ? editedFile(entry) : null;
  if (file !== null && onOpenFile) {
    return (
      <div className="min-w-0">
        <button type="button" title={file} onClick={() => onOpenFile(file)} className={ENTRY_CLASS}>
          <LineGutter>{entryIcon(entry)}</LineGutter>
          <EntryText
            entering={entering}
            {...status}
            className="underline-offset-2 group-hover:underline"
          >
            {entryLabel(entry)}
          </EntryText>
          <GitDiffIcon
            size={12}
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
        <EntryText entering={entering} {...status}>
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

/** A row that can be opened: one line, full width, with room for an icon and a caret. */
export const ENTRY_CLASS =
  "group flex min-h-6 w-full min-w-0 cursor-pointer items-center gap-1.5 rounded-[var(--radius-sm)] border-none bg-transparent px-0 py-0.5 text-left text-[length:var(--fs-md)] text-fg-muted hover:text-fg";

/** The box a row opens into: a command and its output, or a block of reasoning. */
const DETAILS_CLASS =
  "ms-7 mt-0.5 mb-1 max-h-64 overflow-auto overscroll-contain rounded-[var(--radius-sm)] bg-editor px-2 py-1.5 text-[length:var(--fs-sm)] text-fg-muted selectable";

/** The line gutter. A fixed width keeps every label on the same left edge, and the icon sits flush
 * with the line's left edge, the same as the status line's words. */
export function LineGutter({ children }: { children: ReactNode }) {
  return (
    <span className="grid w-6 flex-none items-center justify-items-start opacity-85">
      {children}
    </span>
  );
}

/**
 * The text of a row that has an icon beside it, then how its work stands: the shared spinner while
 * it runs, and a small check once it is done.
 *
 * Nothing here fades. A label that dims and brightens on a loop reads as text that cannot settle,
 * and beside an icon the icon looks like it is going with it. Only a row that has just arrived
 * fades its text in.
 */
export function EntryText({
  entering = false,
  running = false,
  done = false,
  failed = false,
  lines = null,
  result = null,
  className = "",
  children,
}: {
  /** The row has just arrived, so its text fades in. */
  entering?: boolean;
  /** Its work is running, so a spinner follows its text. */
  running?: boolean;
  /** Its work is done, so a check follows its text. */
  done?: boolean;
  /** Its work failed, so a red cross follows its text. */
  failed?: boolean;
  /** The lines its edits added and removed. */
  lines?: ToolCall["lines"];
  /** What its command came to. */
  result?: CommandResult | null;
  className?: string;
  children: ReactNode;
}) {
  return (
    <span
      className={`flex min-w-0 flex-1 items-center gap-2 ${entering ? "timeline-status-enter" : ""} ${className}`}
    >
      <span className="min-w-0 truncate">{children}</span>
      {/* The pill and the mark sit at the row's right edge, so they line up down the list. */}
      <span className="ml-auto flex flex-none items-center gap-2">
        {lines && <LinesPill lines={lines} />}
        {result && <ResultPill result={result} />}
        <span className="grid size-4 flex-none place-items-center">
          {running ? (
            <Spinner size={15} className="text-accent-text" />
          ) : failed ? (
            <span role="img" aria-label="Failed" className="inline-flex text-danger">
              <XIcon size={14} weight="bold" />
            </span>
          ) : (
            done && (
              <span role="img" aria-label="Done" className="inline-flex text-success">
                <CheckIcon size={14} weight="bold" />
              </span>
            )
          )}
        </span>
      </span>
    </span>
  );
}

/** "+18 −2": the lines an edit added and removed, leaving out a side that is zero. */
function LinesPill({ lines }: { lines: NonNullable<ToolCall["lines"]> }) {
  const { added, removed } = lines;
  if (added === 0 && removed === 0) return null;
  return (
    <span
      aria-label={`${added} ${added === 1 ? "line" : "lines"} added, ${removed} removed`}
      className={PILL_CLASS}
    >
      {added > 0 && <span className="text-success">+{added}</span>}
      {removed > 0 && <span className="text-danger">−{removed}</span>}
    </span>
  );
}

/** "24 passed", "2 failed" or "exit 1": what a command came to. */
function ResultPill({ result }: { result: CommandResult }) {
  if ("exit" in result) {
    return <span className={`${PILL_CLASS} text-danger`}>exit {result.exit}</span>;
  }
  const { passed, failed } = result;
  return (
    <span className={PILL_CLASS}>
      {passed > 0 && <span className="text-success">{passed} passed</span>}
      {failed > 0 && <span className="text-danger">{failed} failed</span>}
    </span>
  );
}

/** A small mono tag beside a row's mark. */
const PILL_CLASS =
  "flex flex-none gap-1.5 rounded-[var(--radius-sm)] bg-[color-mix(in_srgb,var(--text)_8%,transparent)] px-1.5 font-[family-name:var(--font-mono)] text-[length:var(--fs-xs)] leading-[18px] tabular-nums";
