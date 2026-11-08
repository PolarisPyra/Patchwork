import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import {
  ArrowCounterClockwiseIcon,
  ArrowLeftIcon,
  ArrowsClockwiseIcon,
  CaretDownIcon,
  CaretRightIcon,
  PlusIcon,
  SparkleIcon,
} from "@phosphor-icons/react";
import { api, onProjectChanged, type FileChange, type GitSyncState } from "~/api";
import { confirm } from "@tauri-apps/plugin-dialog";
import Spinner from "~/components/Spinner";
import { useMissingFiles } from "~/hooks/useMissingFiles";
import { baseName, errorText } from "~/lib/util";
import { ChangeRow } from "~/features/sidebar/components/ChangeRow";
import CommitBox from "~/features/sidebar/components/CommitBox";
import GraphSection from "~/features/sidebar/components/GraphSection";
import PullRequestBar from "~/features/sidebar/components/PullRequestBar";
import type { SidebarProps } from "~/features/sidebar/types";

/** Source Control: the commit box, the changed files, and the commit graph. */

/** The Graph's height while Changes is open too, as the divider between them left it. */
const GRAPH_HEIGHT_KEY = "patchwork.graphHeight";
const GRAPH_HEIGHT = 260;
/** Neither pane is dragged smaller than its header and a few rows. */
const PANE_MIN_PX = 88;

/** A collapsible part of the view, such as Changes or Graph: `height` tall, else it fills what
 * the other pane leaves. */
function Pane({
  title,
  open,
  onToggle,
  height,
  actions,
  children,
}: {
  title: string;
  open: boolean;
  onToggle: () => void;
  height?: number;
  actions?: ReactNode;
  children: ReactNode;
}) {
  const Caret = open ? CaretDownIcon : CaretRightIcon;
  return (
    <section
      aria-label={title}
      style={open && height ? { height, maxHeight: `calc(100% - ${PANE_MIN_PX}px)` } : undefined}
      className={`flex min-h-0 flex-col border-t border-border first:border-t-0 ${open && !height ? "flex-1" : "flex-none"}`}
    >
      <div className="group flex h-[22px] flex-none items-center pr-2">
        <button
          className="flex min-w-0 flex-1 cursor-pointer items-center gap-1 border-none bg-transparent px-1 text-left text-[length:var(--fs-xs)] font-semibold text-fg"
          aria-expanded={open}
          onClick={onToggle}
        >
          <Caret size={12} />
          {title}
        </button>
        {open && actions}
      </div>
      {open && (
        <div className="min-h-0 flex-1 overflow-x-hidden overflow-y-scroll pb-1 [&::-webkit-scrollbar]:w-1 [&::-webkit-scrollbar-thumb]:border-0">
          {children}
        </div>
      )}
    </section>
  );
}

/** A small icon button for a row or group, shown on hover. */
function RowAction({
  label,
  onClick,
  children,
}: {
  label: string;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      className="btn-icon hover-reveal !h-5 !w-5"
      title={label}
      aria-label={label}
      onClick={(e) => {
        e.stopPropagation();
        onClick();
      }}
    >
      {children}
    </button>
  );
}

/** A group of files under its name, a count, and the actions for all of them. */
function Group({
  title,
  count,
  actions,
  children,
}: {
  title: string;
  count: number;
  actions?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div role="group" aria-label={title} className="flex flex-col px-1">
      <div className="group flex h-[22px] items-center gap-1 px-1 text-[length:var(--fs-sm)]">
        <span className="min-w-0 flex-1 truncate">{title}</span>
        {actions}
        <span className="min-w-[18px] rounded-full bg-selected-muted px-1.5 text-center text-[length:var(--fs-xs)] leading-[18px]">
          {count}
        </span>
      </div>
      {children}
    </div>
  );
}

export default function ChangesView({
  project,
  threads,
  selectedId,
  branch,
  changes,
  changeFocus,
  onClearChangeFocus,
  onRefreshChanges,
  onOpenChange,
  archivedThreads,
  onSelectThread,
  onSendToThread,
  onStartTask,
}: SidebarProps) {
  const [error, setError] = useState<string | null>(null);
  const [resolving, setResolving] = useState(false);
  const [sync, setSync] = useState<GitSyncState | null>(null);
  const [changesOpen, setChangesOpen] = useState(true);
  const [graphOpen, setGraphOpen] = useState(true);
  const [graphHeight, setGraphHeight] = useState(
    () => Number(localStorage.getItem(GRAPH_HEIGHT_KEY)) || GRAPH_HEIGHT,
  );
  const panesRef = useRef<HTMLDivElement>(null);
  // With full access nothing waits for review, so staging and discarding are left out.
  const fullAccess = threads.find((t) => t.id === selectedId)?.accessMode === "full-access";
  // The reply's files, listed whether or not they still have uncommitted changes.
  const replyFiles = changeFocus ?? [];
  const missing = useMissingFiles(project?.id ?? null, replyFiles, changes);
  const hasChanges = changes.length > 0;
  const projectId = project?.id ?? null;

  // Where the branch stands asks gh over the network, so it is read when the branch or whether
  // anything is uncommitted changes, and after each action, not with every edit.
  const refreshSync = useCallback(() => {
    if (!projectId) return;
    api.gitSyncState(projectId).then(setSync, () => setSync(null));
  }, [projectId]);
  useEffect(refreshSync, [refreshSync, branch, hasChanges]);
  // Git run elsewhere, such as a push or fetch in a terminal, moves the branch too.
  useEffect(() => {
    if (!projectId) return;
    const unlisten = onProjectChanged((e) => {
      if (e.projectId === projectId && e.paths.some((p) => p.startsWith(".git/"))) refreshSync();
    });
    return () => void unlisten.then((stop) => stop());
  }, [projectId, refreshSync]);

  function refreshAll() {
    onRefreshChanges();
    refreshSync();
  }

  /** The Graph's height kept between the panes' smallest, and remembered. */
  function setGraphHeightClamped(next: number) {
    const total = panesRef.current?.clientHeight ?? 0;
    const height = Math.round(Math.max(PANE_MIN_PX, Math.min(total - PANE_MIN_PX, next)));
    localStorage.setItem(GRAPH_HEIGHT_KEY, String(height));
    setGraphHeight(height);
  }

  function startResize(e: React.MouseEvent) {
    const panes = panesRef.current;
    if (!panes) return;
    e.preventDefault();
    const bottom = panes.getBoundingClientRect().bottom;
    const onMove = (ev: MouseEvent) => setGraphHeightClamped(bottom - ev.clientY);
    const onUp = () => {
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  }

  function resizeByKey(e: React.KeyboardEvent) {
    const step = e.key === "ArrowUp" ? 16 : e.key === "ArrowDown" ? -16 : 0;
    if (!step) return;
    e.preventDefault();
    setGraphHeightClamped(graphHeight + step);
  }

  async function act(action: () => Promise<void>) {
    setError(null);
    try {
      await action();
    } catch (e) {
      setError(errorText(e));
    }
    onRefreshChanges();
  }

  /** Stages every file in `files`, one after another. */
  function stage(files: FileChange[]) {
    if (!project) return;
    void act(async () => {
      for (const f of files) await api.acceptFile(project.id, f.path);
    });
  }

  /** Reverts every file in `files` after one confirmation. */
  async function discard(files: FileChange[]) {
    if (!project || files.length === 0) return;
    const what = files.length === 1 ? baseName(files[0].path) : `${files.length} files`;
    if (!(await confirm(`Discard changes to ${what}?`, { title: "Discard", kind: "warning" }))) {
      return;
    }
    void act(async () => {
      for (const f of files) await api.revertFile(project.id, f.path);
    });
  }

  /** Starts a task whose agent resolves the conflicts, given both sides of each. */
  async function resolveWithAgent() {
    if (!projectId) return;
    setError(null);
    setResolving(true);
    try {
      onStartTask("Resolve merge conflicts", await api.conflictPrompt(projectId));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setResolving(false);
    }
  }

  const row = (c: FileChange) => (
    <ChangeRow
      key={c.path}
      change={c}
      onOpen={() => onOpenChange(c.path)}
      actions={
        !fullAccess && (
          <>
            <RowAction label="Discard changes" onClick={() => void discard([c])}>
              <ArrowCounterClockwiseIcon size={14} />
            </RowAction>
            {!c.staged && (
              <RowAction label="Stage changes" onClick={() => stage([c])}>
                <PlusIcon size={14} />
              </RowAction>
            )}
          </>
        )
      }
    />
  );

  const conflicted = changes.filter((c) => c.status === "conflicted");
  const pinned = changes.filter((c) => replyFiles.includes(c.path) && !conflicted.includes(c));
  const rest = changes.filter((c) => !pinned.includes(c) && !conflicted.includes(c));
  const staged = rest.filter((c) => c.staged);
  const unstaged = rest.filter((c) => !c.staged);

  return (
    <>
      {/* The panes below keep a 4px scrollbar and an 8px inset, so the header ends where they do. */}
      <div className="mt-1.5 flex h-8 flex-none items-center justify-between px-3 text-[length:var(--fs-sm)] font-semibold">
        <span>Source Control</span>
        <button className="btn-icon" title="Refresh" aria-label="Refresh" onClick={refreshAll}>
          {/* The text sits low in its line box, so the icon comes down to its middle. */}
          <ArrowsClockwiseIcon size={18} className="translate-y-px" />
        </button>
      </div>
      <div ref={panesRef} className="flex min-h-0 flex-1 flex-col">
        <Pane title="Changes" open={changesOpen} onToggle={() => setChangesOpen((o) => !o)}>
          {project && (
            <CommitBox
              projectId={project.id}
              threadId={selectedId}
              sync={sync}
              hasChanges={hasChanges}
              onDone={refreshAll}
            />
          )}
          {project && sync?.pullRequest && (
            <PullRequestBar
              projectId={project.id}
              pr={sync.pullRequest}
              threads={threads}
              onSendToThread={onSendToThread}
              onStartTask={onStartTask}
            />
          )}
          {conflicted.length > 0 && (
            <Group
              title="Merge Changes"
              count={conflicted.length}
              actions={
                <button
                  className="flex flex-none cursor-pointer items-center gap-1 rounded-[var(--radius-sm)] border-none bg-transparent px-1.5 py-0.5 text-[length:var(--fs-xs)] text-fg-subtle hover:bg-hover hover:text-fg disabled:cursor-default"
                  disabled={resolving}
                  onClick={() => void resolveWithAgent()}
                >
                  {resolving ? <Spinner size={12} /> : <SparkleIcon size={14} />}
                  Resolve with agent
                </button>
              }
            >
              {conflicted.map(row)}
            </Group>
          )}
          {replyFiles.length > 0 && (
            <Group
              title="From this reply"
              count={replyFiles.length}
              actions={
                <button
                  className="flex flex-none cursor-pointer items-center gap-1 rounded-[var(--radius-sm)] border-none bg-transparent px-1.5 py-0.5 text-[length:var(--fs-xs)] text-fg-subtle hover:bg-hover hover:text-fg"
                  onClick={onClearChangeFocus}
                >
                  <ArrowLeftIcon size={14} />
                  Project
                </button>
              }
            >
              {pinned.map(row)}
              {replyFiles
                .filter((path) => !changes.some((c) => c.path === path))
                .map((path) => (
                  <button
                    key={path}
                    className="flex h-[22px] cursor-pointer items-center gap-1.5 rounded-[var(--radius-sm)] border-none bg-transparent pr-1.5 pl-5 text-left text-[length:var(--fs-sm)] text-fg-muted hover:bg-hover"
                    title={path}
                    onClick={() => onOpenChange(path)}
                  >
                    <span
                      className={`min-w-0 flex-1 truncate ${missing.has(path) ? "line-through" : ""}`}
                    >
                      {baseName(path)}
                    </span>
                    <span className="flex-none text-[length:var(--fs-xs)] text-fg-subtle">
                      {missing.has(path) ? "Deleted" : "No changes"}
                    </span>
                  </button>
                ))}
            </Group>
          )}
          {staged.length > 0 && (
            <Group title="Staged Changes" count={staged.length}>
              {staged.map(row)}
            </Group>
          )}
          {unstaged.length > 0 && (
            <Group
              title="Changes"
              count={unstaged.length}
              actions={
                !fullAccess && (
                  <>
                    <RowAction label="Discard all changes" onClick={() => void discard(unstaged)}>
                      <ArrowCounterClockwiseIcon size={14} />
                    </RowAction>
                    <RowAction label="Stage all changes" onClick={() => stage(unstaged)}>
                      <PlusIcon size={14} />
                    </RowAction>
                  </>
                )
              }
            >
              {unstaged.map(row)}
            </Group>
          )}
          {error && (
            <div role="alert" className="px-3 text-[length:var(--fs-sm)] text-danger">
              {error}
            </div>
          )}
        </Pane>
        {changesOpen && graphOpen && (
          <div
            role="separator"
            aria-orientation="horizontal"
            aria-label="Resize graph"
            aria-valuenow={graphHeight}
            aria-valuemin={PANE_MIN_PX}
            aria-valuemax={
              panesRef.current ? panesRef.current.clientHeight - PANE_MIN_PX : undefined
            }
            tabIndex={0}
            onKeyDown={resizeByKey}
            onMouseDown={startResize}
            className="relative z-[1] -mb-1 h-1 flex-none cursor-row-resize focus-visible:bg-accent"
          />
        )}
        <Pane
          title="Graph"
          open={graphOpen}
          onToggle={() => setGraphOpen((o) => !o)}
          height={changesOpen ? graphHeight : undefined}
        >
          {project && (
            <GraphSection
              projectId={project.id}
              changes={changes}
              threads={threads}
              archivedThreads={archivedThreads}
              onSelectThread={onSelectThread}
            />
          )}
        </Pane>
      </div>
    </>
  );
}
