import { useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  ArrowUpIcon,
  CaretDownIcon,
  CheckIcon,
  CloudArrowUpIcon,
  GitPullRequestIcon,
  SparkleIcon,
} from "@phosphor-icons/react";
import { api, isMac, type GitSyncState, type PullRequest } from "~/api";
import Dropdown from "~/components/Dropdown";
import Spinner from "~/components/Spinner";
import { errorText } from "~/lib/util";
import { ACTION_LABELS, gitActions, type GitAction } from "~/features/sidebar/lib/gitActions";

/** The commit message box and the Commit button with its menu, at the top of Source Control. */

/** Unsent messages by project, kept while the sidebar shows another view. */
const drafts = new Map<string, string>();

/** How long a finished action's note stays. */
const RESULT_MS = 10_000;

const MIN_LINES = 3;
const MAX_LINES = 10;

export default function CommitBox({
  projectId,
  threadId,
  sync,
  hasChanges,
  onDone,
}: {
  projectId: string;
  /** The open chat or task, whose model writes a message when no title model is set. */
  threadId: string | null;
  sync: GitSyncState | null;
  hasChanges: boolean;
  /** Reads git's state again after an action. */
  onDone: () => void;
}) {
  const [message, setMessageState] = useState(() => drafts.get(projectId) ?? "");
  const [busy, setBusy] = useState<string | null>(null);
  const [result, setResult] = useState<{ text: string; pr?: PullRequest } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const input = useRef<HTMLTextAreaElement>(null);

  function setMessage(text: string) {
    drafts.set(projectId, text);
    setMessageState(text);
  }

  useEffect(() => setMessageState(drafts.get(projectId) ?? ""), [projectId]);

  useEffect(() => {
    if (!result) return;
    const timer = setTimeout(() => setResult(null), RESULT_MS);
    return () => clearTimeout(timer);
  }, [result]);

  // Three lines tall, it grows with the message up to ten, then scrolls.
  useLayoutEffect(() => {
    const el = input.current;
    if (!el) return;
    el.style.height = "auto";
    const line = parseFloat(getComputedStyle(el).lineHeight) || 18;
    const height = Math.max(el.scrollHeight, line * MIN_LINES + 8);
    el.style.height = `${Math.min(height, line * MAX_LINES + 8)}px`;
  }, [message]);

  const { primary, menu } = gitActions(sync, hasChanges);

  async function run(action: GitAction) {
    setError(null);
    setResult(null);
    if (action === "viewPr") {
      if (sync?.pullRequest) void api.openUrl(sync.pullRequest.url);
      return;
    }
    let done: { text: string; pr?: PullRequest } | null = null;
    try {
      if (action === "commit" || action === "commitPush" || action === "commitPushPr") {
        setBusy(message.trim() ? "Committing…" : "Writing message…");
        const [hash, subject] = await api.commitChanges(projectId, threadId, message);
        setMessage("");
        done = { text: `Committed ${hash}: ${subject}` };
      }
      if (action === "commitPush" || action === "commitPushPr" || action === "push") {
        setBusy("Pushing…");
        done = { text: `Pushed to ${await api.gitPush(projectId)}` };
      }
      if (action === "commitPushPr" || action === "openPr") {
        setBusy("Opening pull request…");
        const pr = await api.openPullRequest(projectId, threadId);
        done = { text: `Pull request #${pr.number}`, pr };
      }
      setResult(done);
    } catch (e) {
      // What finished before the failure still shows, above the error.
      setResult(done);
      setError(errorText(e));
    } finally {
      setBusy(null);
      onDone();
    }
  }

  async function writeMessage() {
    setError(null);
    setBusy("Writing message…");
    try {
      setMessage(await api.writeCommitMessage(projectId, threadId));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
      onDone();
    }
  }

  const key = isMac ? "Cmd+Enter" : "Ctrl+Enter";
  const branch = sync?.branch;
  const placeholder = branch
    ? `Message (${key} to commit on "${branch}")`
    : `Message (${key} to commit)`;
  const PrimaryIcon =
    primary.label === "Publish Branch"
      ? CloudArrowUpIcon
      : primary.action === "push"
        ? ArrowUpIcon
        : primary.action === "openPr" || primary.action === "viewPr"
          ? GitPullRequestIcon
          : CheckIcon;

  return (
    <div className="flex flex-col gap-2 px-2 pt-1 pb-2">
      <div className="relative flex rounded-[var(--radius-sm)] border border-border bg-input focus-within:border-accent">
        <textarea
          ref={input}
          rows={MIN_LINES}
          value={message}
          aria-label="Commit message"
          placeholder={placeholder}
          className="min-w-0 flex-1 resize-none border-none bg-transparent py-1 pr-7 pl-2 text-[length:var(--fs-sm)] leading-[18px] text-fg outline-none placeholder:text-fg-subtle"
          onChange={(e) => setMessage(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.ctrlKey || e.metaKey) && hasChanges && !busy) {
              e.preventDefault();
              void run("commit");
            }
          }}
        />
        <button
          className="btn-icon absolute top-0.5 right-0.5 !h-6 !w-6"
          title="Write commit message"
          aria-label="Write commit message"
          disabled={!hasChanges || !!busy}
          onClick={() => void writeMessage()}
        >
          <SparkleIcon size={14} />
        </button>
      </div>
      <div className="flex h-7">
        <button
          className="flex min-w-0 flex-1 cursor-pointer items-center justify-center gap-1.5 rounded-l-[var(--radius-sm)] border-none bg-accent px-2 text-[length:var(--fs-sm)] text-on-accent hover:bg-accent-hover disabled:cursor-default disabled:opacity-50"
          disabled={!primary.action || !!busy}
          onClick={() => primary.action && void run(primary.action)}
        >
          {busy ? <Spinner size={12} /> : <PrimaryIcon size={14} />}
          <span className="truncate">{busy ?? primary.label}</span>
        </button>
        <Dropdown
          label="Commit actions"
          alignEnd
          trigger={({ open, toggle }) => (
            <button
              className="flex h-7 w-6 cursor-pointer items-center justify-center rounded-r-[var(--radius-sm)] border-0 border-l border-solid border-l-black/20 bg-accent p-0 text-on-accent hover:bg-accent-hover disabled:cursor-default disabled:opacity-50"
              aria-label="More actions"
              title="More actions"
              aria-haspopup="menu"
              aria-expanded={open}
              disabled={menu.length === 0 || !!busy}
              onClick={toggle}
            >
              <CaretDownIcon size={12} />
            </button>
          )}
        >
          {(close) =>
            menu.map((action) => (
              <button
                key={action}
                role="menuitem"
                className="flex cursor-pointer items-center gap-1.5 rounded-[var(--radius-sm)] border-none bg-transparent px-2 py-[5px] text-left whitespace-nowrap text-fg hover:bg-hover"
                onClick={() => {
                  close();
                  void run(action);
                }}
              >
                {action === "viewPr" && sync?.pullRequest
                  ? `${ACTION_LABELS.viewPr} #${sync.pullRequest.number}`
                  : ACTION_LABELS[action]}
              </button>
            ))
          }
        </Dropdown>
      </div>
      {result && (
        <div
          role="status"
          className="flex items-center gap-1.5 px-0.5 text-[length:var(--fs-xs)] text-fg-muted"
        >
          <span className="min-w-0 truncate" title={result.text}>
            {result.text}
          </span>
          {result.pr && (
            <button
              className="flex-none cursor-pointer border-none bg-transparent p-0 text-[length:var(--fs-xs)] text-accent-text hover:underline"
              onClick={() => void api.openUrl(result.pr!.url)}
            >
              Open
            </button>
          )}
        </div>
      )}
      {error && (
        <div role="alert" className="px-0.5 text-[length:var(--fs-xs)] break-words text-danger">
          {error}
        </div>
      )}
    </div>
  );
}
