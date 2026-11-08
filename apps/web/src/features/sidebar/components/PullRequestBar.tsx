import { useState } from "react";
import { GitPullRequestIcon, PaperPlaneRightIcon } from "@phosphor-icons/react";
import { api, type PullRequest, type Thread } from "~/api";
import Spinner from "~/components/Spinner";
import { errorText } from "~/lib/util";

/** The branch's open pull request under the commit box: its checks, its comments, and sending
 * what failed or was said back to the agent. */
export default function PullRequestBar({
  projectId,
  pr,
  threads,
  onSendToThread,
  onStartTask,
}: {
  projectId: string;
  pr: PullRequest;
  threads: Thread[];
  onSendToThread: (threadId: string, text: string) => void;
  onStartTask: (title: string, text: string) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const checks =
    pr.failed > 0
      ? `${pr.failed} failed`
      : pr.pending > 0
        ? `${pr.pending} running`
        : pr.passed > 0
          ? "Checks passed"
          : null;
  const comments =
    pr.comments > 0 ? `${pr.comments} ${pr.comments === 1 ? "comment" : "comments"}` : null;

  /** Sends to the thread that made the branch's newest commit, or starts a task when none did. */
  async function send() {
    setError(null);
    setBusy(true);
    try {
      const [threadId, prompt] = await api.pullRequestFeedback(projectId);
      if (threadId && threads.some((t) => t.id === threadId)) onSendToThread(threadId, prompt);
      else onStartTask(`Pull request #${pr.number} feedback`, prompt);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex flex-col gap-1 px-2 pb-2">
      <div className="flex h-6 items-center gap-1.5 text-[length:var(--fs-xs)] text-fg-muted">
        <button
          className="flex flex-none cursor-pointer items-center gap-1 border-none bg-transparent p-0 text-[length:var(--fs-xs)] text-fg hover:underline"
          title={pr.url}
          onClick={() => void api.openUrl(pr.url)}
        >
          <GitPullRequestIcon size={14} />#{pr.number}
        </button>
        <span role="status" className="min-w-0 flex-1 truncate">
          {checks && <span className={pr.failed > 0 ? "text-danger" : undefined}>{checks}</span>}
          {checks && comments && " · "}
          {comments}
        </span>
        {(pr.failed > 0 || pr.comments > 0) && (
          <button
            className="flex flex-none cursor-pointer items-center gap-1 rounded-[var(--radius-sm)] border-none bg-transparent px-1.5 py-0.5 text-[length:var(--fs-xs)] text-fg-subtle hover:bg-hover hover:text-fg disabled:cursor-default"
            disabled={busy}
            onClick={() => void send()}
          >
            {busy ? <Spinner size={12} /> : <PaperPlaneRightIcon size={14} />}
            Send to agent
          </button>
        )}
      </div>
      {error && (
        <div role="alert" className="px-0.5 text-[length:var(--fs-xs)] break-words text-danger">
          {error}
        </div>
      )}
    </div>
  );
}
