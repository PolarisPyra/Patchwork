import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  ArrowSquareOutIcon,
  CheckIcon,
  PlusIcon,
  CodeIcon,
  CopyIcon,
  XIcon,
} from "@phosphor-icons/react";
import Spinner from "~/components/Spinner";
import { api, onProjectChanged, type FileChange, type Message } from "~/api";
import CodeView from "~/components/canvas/CodeView";
import DiffView from "~/components/canvas/DiffView";
import PlanView from "~/components/canvas/PlanView";
import { describeFile, splitLines } from "~/components/canvas/highlight";
import { copyText } from "~/lib/clipboard";

export type CanvasTab = "plan" | "code" | "diff";

export interface CanvasProps {
  projectId: string;
  /** Project-relative file path to show in Code/Diff tabs. */
  path: string | null;
  /** Assistant message whose plan (Plan tab) and files are shown; null if none. */
  message: Message | null;
  initialTab?: CanvasTab;
  /** The project's uncommitted changes, which the workspace keeps current. */
  changes: FileChange[];
  onClose: () => void;
  /** Called after Apply/Reject succeeds so the parent can refresh git state. */
  onChangesUpdated: () => void;
}

/** A load result tagged with the project+path it belongs to, so stale results are ignored. */
interface Loaded<T> {
  key: string;
  value?: T;
  error?: string;
}

function errorMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export default function Canvas({
  projectId,
  path,
  message,
  initialTab,
  changes,
  onClose,
  onChangesUpdated,
}: CanvasProps) {
  const [tab, setTab] = useState<CanvasTab>(initialTab ?? (path ? "code" : "plan"));
  const [file, setFile] = useState<Loaded<string> | null>(null);
  const [diffResult, setDiffResult] = useState<Loaded<string> | null>(null);
  const [reloadToken, setReloadToken] = useState(0);
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const key = `${projectId}\u0000${path ?? ""}`;
  const activeTab: CanvasTab = path === null ? "plan" : tab;

  useEffect(() => {
    setTab(initialTab ?? (path ? "code" : "plan"));
  }, [path, initialTab]);

  // Reload content and diff whenever the file changes or a reload is requested.
  useEffect(() => {
    let live = true;
    if (path) {
      api.readFile(projectId, path).then(
        (value) => {
          if (live) setFile({ key, value });
        },
        (e: unknown) => {
          if (live) setFile({ key, error: errorMessage(e) });
        },
      );
      // A file the reply changed shows what its turn did, which stays after a commit.
      const turn = message?.turn && message.files.includes(path) ? message.turn : null;
      (turn ? api.turnFileDiff(projectId, turn, path) : api.gitFileDiff(projectId, path)).then(
        (value) => {
          if (live) setDiffResult({ key, value });
        },
        (e: unknown) => {
          if (live) setDiffResult({ key, error: errorMessage(e) });
        },
      );
    }
    return () => {
      live = false;
    };
  }, [key, projectId, path, reloadToken, message?.turn, message?.files]);

  // The workspace's watcher reports settled bursts of changes. Reload when one touched this file,
  // or `.git`, since staging and commits change its diff.
  useEffect(() => {
    if (!path) return;
    const unlisten = onProjectChanged((e) => {
      if (e.projectId !== projectId) return;
      if (e.paths.some((p) => p === path || p.startsWith(".git/"))) setReloadToken((n) => n + 1);
    });
    return () => void unlisten.then((stop) => stop());
  }, [projectId, path]);

  // The agent may have edited the file while it was streaming.
  const status = message?.status;
  const prevStatus = useRef(status);
  useEffect(() => {
    if (prevStatus.current === "streaming" && status !== "streaming") {
      setReloadToken((n) => n + 1);
    }
    prevStatus.current = status;
  }, [status]);

  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(false), 1500);
    return () => window.clearTimeout(timer);
  }, [copied]);

  const code = file?.key === key ? file : null;
  const diff = diffResult?.key === key ? diffResult : null;
  const change = changes.find((c) => c.path === path);

  const name = path ? (path.split("/").pop() ?? path) : "No file selected";
  const kind = path ? describeFile(path) : null;
  const dir = path && path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : null;
  const content = code?.value;
  const lineCount = content === undefined ? null : splitLines(content).length;
  const planCount = message?.plan.length ?? 0;
  const diffCount =
    message?.turn && path && message.files.includes(path)
      ? (diff?.value ?? "").split("\n").filter((l) => /^[+-]/.test(l) && !/^(\+\+\+|---) /.test(l))
          .length
      : change
        ? change.additions + change.deletions
        : 0;

  const tabs: { id: CanvasTab; label: string; count: number }[] = path
    ? [
        { id: "plan", label: "Plan", count: planCount },
        { id: "code", label: "Code", count: 0 },
        { id: "diff", label: "Diff", count: diffCount },
      ]
    : [{ id: "plan", label: "Plan", count: planCount }];

  async function runAction(action: () => Promise<void>) {
    setBusy(true);
    setActionError(null);
    try {
      await action();
      onChangesUpdated();
      setReloadToken((n) => n + 1);
    } catch (e) {
      setActionError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  function openInEditor() {
    if (!path) return;
    setActionError(null);
    api.openInEditor(projectId, path).catch((e: unknown) => setActionError(errorMessage(e)));
  }

  async function copyContent() {
    if (content === undefined) return;
    try {
      await copyText(content);
      setCopied(true);
    } catch (e) {
      setActionError(errorMessage(e));
    }
  }

  function apply() {
    if (!path) return;
    void runAction(() => api.acceptFile(projectId, path));
  }

  function reject() {
    if (!path || !window.confirm(`Discard changes to ${name}?`)) return;
    void runAction(() => api.revertFile(projectId, path));
  }

  function renderBody(): ReactNode {
    if (activeTab === "plan" || !path) {
      return <PlanView message={message} />;
    }
    const result = activeTab === "code" ? code : diff;
    if (!result || (result.value === undefined && result.error === undefined)) {
      return (
        <div className="flex items-center justify-center h-full min-h-[120px] p-6 text-center text-fg-subtle">
          <Spinner size={18} />
        </div>
      );
    }
    if (result.error !== undefined) {
      return (
        <div
          className="flex items-center justify-center h-full min-h-[120px] p-6 text-center text-fg-muted"
          title={result.error}
        >
          {activeTab === "code"
            ? "This file can't be displayed. It may have been deleted or is binary."
            : "The diff for this file could not be loaded."}
        </div>
      );
    }
    return activeTab === "code" ? (
      <CodeView path={path} content={result.value ?? ""} />
    ) : (
      <DiffView diff={result.value ?? ""} />
    );
  }

  return (
    <aside className="flex flex-col h-full min-h-0 bg-editor border-l border-border">
      <header className="flex items-center gap-3 flex-none py-3.5 px-5 border-b border-border">
        <div className="flex items-center justify-center flex-none size-9 rounded-[8px] bg-accent-soft text-accent-text">
          <CodeIcon size={18} />
        </div>
        <div className="flex-1 min-w-0">
          <div
            className="text-[length:var(--fs-lg)] font-semibold whitespace-nowrap overflow-hidden text-ellipsis select-text"
            title={path ?? undefined}
          >
            {name}
          </div>
          {kind && (
            <div className="truncate text-[length:var(--fs-sm)] text-fg-muted tabular-nums select-text">
              {[dir, kind.label, lineCount === null ? null : `${lineCount} lines`]
                .filter(Boolean)
                .join(" · ")}
            </div>
          )}
        </div>
        <div className="flex items-center gap-2 flex-none">
          {path && (
            <>
              <button className="btn" onClick={openInEditor}>
                <ArrowSquareOutIcon size={14} />
                Open in editor
              </button>
              <button
                className="btn"
                onClick={() => void copyContent()}
                disabled={content === undefined}
              >
                {copied ? <CheckIcon size={14} /> : <CopyIcon size={14} />}
                {copied ? "Copied" : "Copy"}
              </button>
            </>
          )}
          <button
            className="btn btn-icon"
            onClick={onClose}
            aria-label="Close canvas"
            title="Close"
          >
            <XIcon size={16} />
          </button>
        </div>
      </header>

      <nav
        className="flex items-center gap-1 flex-none h-10 px-3 border-b border-border overflow-x-auto"
        role="tablist"
      >
        {tabs.map((t) => (
          <button
            key={t.id}
            role="tab"
            aria-selected={activeTab === t.id}
            className="inline-flex items-center gap-1.5 h-7 px-2.5 border-none rounded-[var(--radius)] bg-transparent text-fg-muted! hover:text-fg! cursor-pointer whitespace-nowrap aria-selected:text-accent-text! aria-selected:bg-accent-soft"
            onClick={() => setTab(t.id)}
          >
            {t.label}
            {t.count > 0 && <span className="badge tabular-nums">{t.count}</span>}
          </button>
        ))}
      </nav>

      <div className="flex-1 min-h-0 overflow-auto selectable">{renderBody()}</div>

      {actionError && (
        <div
          className="flex-none py-2 px-5 border-t border-border text-danger text-[length:var(--fs-sm)] select-text"
          role="alert"
        >
          {actionError}
        </div>
      )}

      {path && change && (
        <footer className="flex items-center justify-between gap-2 flex-none py-3 px-5 border-t border-border bg-panel">
          <div className="flex items-center gap-1.5 min-w-0 text-[length:var(--fs-sm)] tabular-nums">
            {change.staged ? (
              <>
                <CheckIcon size={14} className="text-success" />
                <span className="font-semibold text-success">Changes applied</span>
              </>
            ) : (
              <span className="text-fg-muted">
                Unstaged changes · +{change.additions} −{change.deletions}
              </span>
            )}
          </div>
          <div className="flex gap-2 flex-none">
            <button className="btn" onClick={reject} disabled={busy}>
              Reject
            </button>
            <button className="btn btn-success" onClick={apply} disabled={busy}>
              <PlusIcon size={14} />
              Stage file
            </button>
          </div>
        </footer>
      )}
    </aside>
  );
}
