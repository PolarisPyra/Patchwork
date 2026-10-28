import { GitDiffIcon } from "@phosphor-icons/react";
import { type FileChange, type Message, type TurnFile, type TurnFileState } from "~/api";
import type { ReplyFiles } from "~/hooks/useReplyFiles";
import { baseName, dirName } from "~/lib/util";

// Markdown loads with the chat, not on first use: its plain-text placeholder had another height,
// so swapping it in moved the messages.

/** Most files the summary lists; the rest are in the Changes view. */
const SUMMARY_FILES = 5;

/** How a file the turn changed stands now, when that is worth a note. */
const TURN_NOTES: Record<TurnFileState, string | null> = {
  uncommitted: null,
  committed: "Committed",
  reverted: "Reverted",
  deleted: "Deleted",
  unchanged: "No changes",
};

/** The files a message touched, each opening its diff. A reply with snapshots of its turn keeps
 * what the turn did, line counts included, once its changes are committed or edited again, and
 * notes where each file stands now. Older replies show the files' uncommitted changes. */
export function ChangeSummary({
  turn,
  paths,
  changes,
  files,
  onOpen,
  onShow,
}: {
  turn: Message["turn"];
  paths: string[];
  changes: FileChange[];
  files: ReplyFiles;
  onOpen: (path: string) => void;
  onShow: () => void;
}) {
  const { turnFiles, missing } = files;
  const found: (FileChange | TurnFile)[] =
    turnFiles ?? changes.filter((c) => paths.includes(c.path));
  const additions = found.reduce((sum, c) => sum + c.additions, 0);
  const deletions = found.reduce((sum, c) => sum + c.deletions, 0);
  const listed = paths.length > SUMMARY_FILES + 1 ? paths.slice(0, SUMMARY_FILES) : paths;
  return (
    <div className="flex flex-col gap-1 rounded-[var(--radius)] border border-border bg-panel px-3 py-2 text-[length:var(--fs-sm)]">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        <GitDiffIcon size={14} className="flex-none text-fg-muted" />
        <span className="whitespace-nowrap">
          {paths.length} {paths.length === 1 ? "file" : "files"} changed
        </span>
        {additions + deletions > 0 && (
          <>
            <span className="text-success">+{additions}</span>
            <span className="text-danger">−{deletions}</span>
          </>
        )}
        <span className="flex-1" />
        <button className="btn flex-none" onClick={onShow}>
          Show changes
        </button>
      </div>
      {/* One grid for all rows, so every file's folder starts in the same column. */}
      <div className="grid grid-cols-[minmax(0,max-content)_minmax(0,1fr)_auto_auto] gap-x-2">
        {listed.map((path) => {
          const file = turnFiles?.find((f) => f.path === path);
          if (turn) {
            return (
              <ChangedFile
                key={path}
                path={path}
                note={file ? TURN_NOTES[file.state] : null}
                lines={file}
                deleted={file?.state === "deleted"}
                onOpen={() => onOpen(path)}
              />
            );
          }
          const change = changes.find((c) => c.path === path);
          const deleted = missing.has(path) || change?.status === "deleted";
          return (
            <ChangedFile
              key={path}
              path={path}
              note={deleted ? "Deleted" : change ? null : "No changes"}
              deleted={deleted}
              onOpen={() => onOpen(path)}
            />
          );
        })}
        {listed.length < paths.length && (
          <button
            className="col-span-full flex min-h-6 cursor-pointer items-center rounded-[var(--radius-sm)] border-none bg-transparent px-1 text-left text-fg-muted hover:bg-hover hover:text-fg"
            onClick={onShow}
          >
            {paths.length - listed.length} more
          </button>
        )}
      </div>
    </div>
  );
}

/** One touched file: its name, struck through once deleted, then its folder, and a note on where
 * it stands, such as "Committed". */
function ChangedFile({
  path,
  note,
  lines,
  deleted,
  onOpen,
}: {
  path: string;
  note: string | null;
  /** Lines the turn added and removed, for replies with snapshots of their turn. */
  lines?: { additions: number; deletions: number };
  deleted: boolean;
  onOpen: () => void;
}) {
  const dir = dirName(path);
  return (
    <button
      className="col-span-full grid min-h-6 min-w-0 cursor-pointer grid-cols-subgrid items-center rounded-[var(--radius-sm)] border-none bg-transparent px-1 text-left hover:bg-hover"
      title={path}
      onClick={onOpen}
    >
      <span className={`truncate ${deleted ? "text-fg-muted line-through" : "text-fg"}`}>
        {baseName(path)}
      </span>
      <span className="truncate text-fg-subtle">{dir}</span>
      {lines && lines.additions + lines.deletions > 0 && (
        <span className="col-start-3 flex gap-1.5 text-[length:var(--fs-xs)] tabular-nums">
          <span className="text-success">+{lines.additions}</span>
          <span className="text-danger">−{lines.deletions}</span>
        </span>
      )}
      {note && (
        <span className="col-start-4 text-[length:var(--fs-xs)] text-fg-subtle">{note}</span>
      )}
    </button>
  );
}
