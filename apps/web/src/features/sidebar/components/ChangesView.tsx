import { useState, type CSSProperties } from "react";
import {
  ArrowLeftIcon,
  ArrowsClockwiseIcon,
  ChatCircleIcon,
  FolderIcon,
} from "@phosphor-icons/react";
import { api, type FileChange } from "~/api";
import { confirm } from "@tauri-apps/plugin-dialog";
import { useMissingFiles } from "~/hooks/useMissingFiles";
import { baseName, dirName, errorText } from "~/lib/util";
import { BulkButton, ChangeRow, REPLY_ROW_CLASS } from "~/features/sidebar/components/ChangeRow";
import type { SidebarProps } from "~/features/sidebar/types";

/** The changes view: the reply's files pinned on top, the rest grouped by folder. */

/** Changed files. Those from the reply the view was opened from are pinned on top; the rest are grouped by folder. */
export default function ChangesView({
  project,
  threads,
  selectedId,
  changes,
  changeFocus,
  onClearChangeFocus,
  onRefreshChanges,
  onOpenChange,
}: SidebarProps) {
  const [error, setError] = useState<string | null>(null);
  // With full access nothing waits for review, so the bulk actions are left out.
  const fullAccess = threads.find((t) => t.id === selectedId)?.accessMode === "full-access";
  // The reply's files, listed whether or not they still have uncommitted changes.
  const replyFiles = changeFocus ?? [];
  const missing = useMissingFiles(project?.id ?? null, replyFiles, changes);
  // The +/− counts sit in columns as wide as the longest count, so they line up across rows.
  const diffDigits = Math.max(
    1,
    ...changes.map((c) => String(Math.max(c.additions, c.deletions)).length),
  );
  const pinned = changes.filter((c) => replyFiles.includes(c.path));
  const folders = new Map<string, FileChange[]>();
  for (const c of changes) {
    if (pinned.includes(c)) continue;
    const dir = dirName(c.path);
    folders.set(dir, [...(folders.get(dir) ?? []), c]);
  }

  async function act(action: () => Promise<void>) {
    setError(null);
    try {
      await action();
      onRefreshChanges();
    } catch (e) {
      setError(errorText(e));
    }
  }

  /** Stages every file in `files`, one after another. */
  function applyAll(files: FileChange[]) {
    if (!project) return;
    void act(async () => {
      for (const f of files) await api.acceptFile(project.id, f.path);
    });
  }

  /** Reverts every file in `files` after one confirmation. */
  async function rejectAll(files: FileChange[]) {
    if (!project || files.length === 0) return;
    const count = files.length === 1 ? "1 file" : `${files.length} files`;
    if (!(await confirm(`Discard changes to ${count}?`, { title: "Reject", kind: "warning" }))) {
      return;
    }
    void act(async () => {
      for (const f of files) await api.revertFile(project.id, f.path);
    });
  }

  const row = (c: FileChange) => (
    <ChangeRow key={c.path} change={c} onOpen={() => onOpenChange(c.path)} />
  );

  return (
    <>
      {/* The list below reserves its 10px scrollbar (theme.css), so the header keeps the same inset. */}
      <div className="flex h-8 flex-none items-center justify-between pr-[18px] pl-3 text-[length:var(--fs-sm)] font-semibold">
        <span>Changes</span>
        <button
          className="btn-icon"
          title="Refresh"
          aria-label="Refresh"
          onClick={onRefreshChanges}
        >
          <ArrowsClockwiseIcon size={18} />
        </button>
      </div>
      <div
        className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-scroll p-2 [scrollbar-gutter:stable]"
        style={{ "--diff-col": `${diffDigits + 1}ch` } as CSSProperties}
      >
        {changes.length > 0 && !fullAccess && (
          <div className="flex justify-end gap-1 px-1">
            <BulkButton onClick={() => applyAll(changes)}>Stage all</BulkButton>
            <BulkButton onClick={() => void rejectAll(changes)}>Reject all</BulkButton>
          </div>
        )}
        {changes.length === 0 && (
          <div className="-mt-2.5 flex h-[22px] items-center px-1 text-[length:var(--fs-sm)] text-fg-muted">
            No changes
          </div>
        )}
        {replyFiles.length > 0 && (
          // Shared columns, so every folder starts together. A name takes at most about half the
          // row and truncates past that; the folder takes what is left, and shrinks first as the
          // sidebar narrows.
          <div
            role="group"
            aria-label="From this reply"
            className="grid grid-cols-[fit-content(55%)_minmax(0,1fr)_auto] gap-x-3 gap-y-px"
          >
            <div className="col-span-full flex items-center gap-1.5 px-1.5 pt-1 text-[length:var(--fs-xs)] text-fg-subtle">
              <ChatCircleIcon size={14} />
              <span className="min-w-0 flex-1 truncate">From this reply</span>
              {pinned.length > 0 && !fullAccess && (
                <span className="flex gap-1">
                  <BulkButton onClick={() => applyAll(pinned)}>Stage</BulkButton>
                  <BulkButton onClick={() => void rejectAll(pinned)}>Reject</BulkButton>
                </span>
              )}
              <button
                className="ml-auto flex flex-none cursor-pointer items-center gap-1 rounded-[var(--radius-sm)] border-none bg-transparent px-1.5 py-0.5 text-[length:var(--fs-xs)] text-fg-subtle hover:bg-hover hover:text-fg"
                onClick={onClearChangeFocus}
              >
                <ArrowLeftIcon size={14} />
                Project
              </button>
            </div>
            {replyFiles.map((path) => {
              const change = changes.find((c) => c.path === path);
              return change ? (
                <ChangeRow key={path} change={change} showDir onOpen={() => onOpenChange(path)} />
              ) : (
                <button
                  key={path}
                  className={`${REPLY_ROW_CLASS} !text-fg-muted`}
                  title={path}
                  onClick={() => onOpenChange(path)}
                >
                  <span className={`truncate ${missing.has(path) ? "line-through" : ""}`}>
                    {baseName(path)}
                  </span>
                  <span className="truncate text-[length:var(--fs-xs)] text-fg-subtle">
                    {dirName(path)}
                  </span>
                  <span className="justify-self-end text-[length:var(--fs-xs)] text-fg-subtle">
                    {missing.has(path) ? "Deleted" : "No changes"}
                  </span>
                </button>
              );
            })}
          </div>
        )}
        {[...folders]
          .sort(([a], [b]) => a.localeCompare(b))
          .map(([dir, files]) => (
            <div key={dir} className="flex flex-col gap-px">
              <div className="flex items-center gap-1.5 px-1.5 pt-1 text-[length:var(--fs-xs)] text-fg-subtle">
                <FolderIcon size={14} />
                <span className="min-w-0 truncate">{dir || "Project root"}</span>
              </div>
              {files.map(row)}
            </div>
          ))}
        {error && (
          <div role="alert" className="px-2 text-[length:var(--fs-sm)] text-danger">
            {error}
          </div>
        )}
      </div>
    </>
  );
}
