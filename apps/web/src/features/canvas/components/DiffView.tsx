import { useMemo, useRef, useState } from "react";
import type { HunkAction } from "~/api";
import { columns, LineBlock, toBlocks } from "~/features/canvas/components/LineBlocks";
import { NoteCard, NoteEditor, SelectionNote } from "~/features/canvas/components/ReviewNotes";
import {
  addNote,
  editNote,
  removeNote,
  useReview,
  type ReviewNote,
} from "~/features/canvas/review";

interface DiffRow {
  kind: "hunk" | "add" | "del" | "ctx";
  text: string;
  /** The change it belongs to, by its place in the diff. */
  hunk: number;
  oldNo?: number;
  newNo?: number;
}

const HUNK_RE = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/;

/** Parses a single-file unified diff into its rows, and each change's text from its `@@` line on,
 * as git takes it back. Header lines are dropped. */
function parseDiff(diff: string): { rows: DiffRow[]; hunks: string[] } {
  const rows: DiffRow[] = [];
  const hunks: string[] = [];
  // The lines of the change being read; null in a file's header.
  let lines: string[] | null = null;
  let oldNo = 0;
  let newNo = 0;
  const end = () => {
    if (lines) hunks.push(lines.join("\n"));
    lines = null;
  };
  for (const line of diff.split("\n")) {
    const header = HUNK_RE.exec(line);
    if (header) {
      end();
      oldNo = Number(header[1]);
      newNo = Number(header[2]);
      lines = [line];
      rows.push({ kind: "hunk", text: line, hunk: hunks.length });
      continue;
    }
    // Another file's part, as in a repository without commits, starts with a header of its own.
    if (line.startsWith("diff --git ")) end();
    if (!lines) continue;
    const hunk = hunks.length;
    if (line.startsWith("+")) {
      rows.push({ kind: "add", text: line.slice(1), hunk, newNo });
      newNo += 1;
    } else if (line.startsWith("-")) {
      rows.push({ kind: "del", text: line.slice(1), hunk, oldNo });
      oldNo += 1;
    } else if (line.startsWith(" ")) {
      rows.push({ kind: "ctx", text: line.slice(1), hunk, oldNo, newNo });
      oldNo += 1;
      newNo += 1;
    } else if (!line.startsWith("\\")) {
      // Trailing blank lines fall through; "\ No newline at end of file" stays in the change.
      continue;
    }
    lines.push(line);
  }
  end();
  return { rows, hunks };
}

const MARKERS: Record<Exclude<DiffRow["kind"], "hunk">, string> = {
  add: "+",
  del: "−",
  ctx: " ",
};

const ROW_BG: Record<Exclude<DiffRow["kind"], "hunk">, string> = {
  add: "bg-[color:var(--diff-add-bg)]",
  del: "bg-[color:var(--diff-del-bg)]",
  ctx: "",
};

/** A note's place and lines, from the rows `from` to `to`; null when they are only headers. */
function noteFor(rows: DiffRow[], from: number, to: number) {
  const picked = rows.slice(from, to + 1).filter((r) => r.kind !== "hunk");
  const last = picked.at(-1);
  if (!last) return null;
  const span = (nos: number[]) =>
    nos[0] === nos.at(-1) ? `line ${nos[0]}` : `lines ${nos[0]}–${nos.at(-1)}`;
  const added = picked.filter((r) => r.kind !== "del").map((r) => r.newNo ?? 0);
  const removed = picked.map((r) => r.oldNo ?? 0).filter((n) => n > 0);
  return {
    where: added.length > 0 ? span(added) : `removed ${span(removed)}`,
    anchor:
      last.kind === "del"
        ? { side: "old" as const, line: last.oldNo ?? 0 }
        : { side: "new" as const, line: last.newNo ?? 0 },
    excerpt: picked.map((r) => (r.kind === "add" ? "+" : r.kind === "del" ? "-" : " ") + r.text),
  };
}

/** Whether `note` shows under `row`. */
const anchoredAt = (note: ReviewNote, row: DiffRow) =>
  note.anchor.side === "old"
    ? row.kind === "del" && row.oldNo === note.anchor.line
    : row.kind !== "del" && row.kind !== "hunk" && row.newNo === note.anchor.line;

type Draft = NonNullable<ReturnType<typeof noteFor>> & { row: number };

/** One file's diff. Each change can be staged, unstaged or discarded, and notes for the agent go
 * under the lines they are about: select lines and press Note, or use a change's Note button. */
export default function DiffView({
  diff,
  projectId,
  path,
  staged,
  busy = false,
  onHunk,
}: {
  diff: string;
  projectId?: string;
  path?: string;
  /** Which changes are staged, in order; missing ones count as unstaged. */
  staged?: boolean[];
  busy?: boolean;
  /** Acts on one change, given as its text; without it the changes have no buttons. */
  onHunk?: (hunk: string, action: HunkAction) => void;
}) {
  const { rows, hunks } = useMemo(() => parseDiff(diff), [diff]);
  const blocks = useMemo(() => toBlocks(rows.map((row, index) => ({ row, index }))), [rows]);
  const root = useRef<HTMLDivElement>(null);
  const review = useReview(projectId ?? "");
  const notes = useMemo(() => review.filter((n) => n.path === path), [review, path]);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const canNote = projectId !== undefined && path !== undefined;
  // The widest line, as a floor for the view's width: blocks off screen are not measured.
  const minWidth = useMemo(() => {
    let line = 0;
    let hunk = 0;
    for (const row of rows) {
      if (row.kind === "hunk") hunk = Math.max(hunk, columns(row.text));
      else line = Math.max(line, columns(row.text));
    }
    // Rows have two 48px number columns and a 20px marker; hunk headers have 16px padding each side.
    return `max(100%, calc(${line}ch + 116px), calc(${hunk}ch + 32px))`;
  }, [rows]);
  // Notes whose lines are no longer in the diff show above it.
  const unplaced = useMemo(
    () => notes.filter((n) => !rows.some((row) => anchoredAt(n, row))),
    [notes, rows],
  );

  if (rows.length === 0) {
    return (
      <div className="flex items-center justify-center h-full min-h-[120px] p-6 text-center text-fg-muted">
        No changes in this file.
      </div>
    );
  }

  function startNote(from: number, to: number) {
    const note = noteFor(rows, from, to);
    // The editor opens under the last line, where the note will show.
    let row = to;
    while (row > from && rows[row].kind === "hunk") row--;
    if (note) setDraft({ ...note, row });
  }

  /** Starts a note on the rows a selection covers. */
  function noteSelection(range: Range) {
    const picked = [...(root.current?.querySelectorAll<HTMLElement>("[data-row]") ?? [])]
      .filter((el) => range.intersectsNode(el))
      .map((el) => Number(el.dataset.row));
    if (picked.length > 0) startNote(Math.min(...picked), Math.max(...picked));
  }

  const card = (note: ReviewNote) =>
    editing === note.id ? (
      <NoteEditor
        key={note.id}
        where={note.where}
        initial={note.text}
        onSave={(text) => {
          if (projectId) editNote(projectId, note.id, text);
          setEditing(null);
        }}
        onCancel={() => setEditing(null)}
      />
    ) : (
      <NoteCard
        key={note.id}
        note={note}
        onEdit={() => setEditing(note.id)}
        onRemove={() => projectId && removeNote(projectId, note.id)}
      />
    );

  return (
    <div
      ref={root}
      className="relative w-max min-w-full py-3 font-[family-name:var(--font-mono)] text-[length:var(--fs-md)] leading-[1.65] text-fg tab-2 select-text"
      style={{ minWidth }}
    >
      {canNote && <SelectionNote root={root} onNote={noteSelection} />}
      {unplaced.map(card)}
      {blocks.map((block, b) => (
        <LineBlock
          key={b}
          rows={block.length}
          extraPx={block.filter(({ row }) => row.kind === "hunk").length * 8}
        >
          {block.map(({ row, index }) => (
            <div key={index}>
              {row.kind === "hunk" ? (
                <div
                  data-hunk={row.hunk}
                  className="flex items-center gap-2 bg-panel py-1 pl-4 text-fg-subtle"
                >
                  <span className="flex-1 whitespace-pre">{row.text}</span>
                  {(onHunk || canNote) && (
                    <span className="sticky right-0 flex flex-none items-center gap-1 bg-panel pr-3 pl-2 font-[family-name:var(--font-sans)] text-[length:var(--fs-xs)]">
                      {canNote && (
                        <HunkButton
                          label={`Add a note on change ${row.hunk + 1}`}
                          onClick={() => {
                            let end = index;
                            while (rows[end + 1]?.hunk === row.hunk) end++;
                            startNote(index + 1, end);
                          }}
                        >
                          Note
                        </HunkButton>
                      )}
                      {onHunk && (
                        <>
                          <HunkButton
                            label={`Discard change ${row.hunk + 1}`}
                            disabled={busy}
                            onClick={() => onHunk(hunks[row.hunk], "discard")}
                          >
                            Discard
                          </HunkButton>
                          {staged?.[row.hunk] ? (
                            <HunkButton
                              label={`Unstage change ${row.hunk + 1}`}
                              disabled={busy}
                              onClick={() => onHunk(hunks[row.hunk], "unstage")}
                            >
                              Unstage
                            </HunkButton>
                          ) : (
                            <HunkButton
                              label={`Stage change ${row.hunk + 1}`}
                              disabled={busy}
                              onClick={() => onHunk(hunks[row.hunk], "stage")}
                            >
                              Stage
                            </HunkButton>
                          )}
                        </>
                      )}
                    </span>
                  )}
                </div>
              ) : (
                <div className={`flex ${ROW_BG[row.kind]}`} data-row={index}>
                  <span className="flex-none min-w-[48px] pr-2 text-right text-fg-subtle select-none tabular-nums">
                    {row.oldNo ?? ""}
                  </span>
                  <span className="flex-none min-w-[48px] pr-2 text-right text-fg-subtle select-none tabular-nums">
                    {row.newNo ?? ""}
                  </span>
                  <span className="flex-none w-5 text-center text-fg-subtle select-none">
                    {MARKERS[row.kind]}
                  </span>
                  <span className="flex-[1_0_auto] whitespace-pre">{row.text}</span>
                </div>
              )}
              {notes.filter((n) => anchoredAt(n, row)).map(card)}
              {draft?.row === index && projectId && path && (
                <NoteEditor
                  where={draft.where}
                  onSave={(text) => {
                    addNote(projectId, {
                      path,
                      where: draft.where,
                      anchor: draft.anchor,
                      excerpt: draft.excerpt,
                      text,
                    });
                    setDraft(null);
                  }}
                  onCancel={() => setDraft(null)}
                />
              )}
            </div>
          ))}
        </LineBlock>
      ))}
    </div>
  );
}

function HunkButton({
  label,
  disabled,
  onClick,
  children,
}: {
  label: string;
  disabled?: boolean;
  onClick: () => void;
  children: string;
}) {
  return (
    <button
      className="cursor-pointer rounded-[var(--radius-sm)] border-none bg-transparent px-1.5 py-0.5 text-fg-muted hover:bg-hover hover:text-fg disabled:cursor-default disabled:opacity-50"
      aria-label={label}
      title={label}
      disabled={disabled}
      onClick={onClick}
    >
      {children}
    </button>
  );
}
