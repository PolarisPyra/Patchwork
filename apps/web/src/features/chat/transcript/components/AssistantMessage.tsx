import { memo } from "react";
import { ArrowClockwiseIcon, GitForkIcon, ShieldWarningIcon } from "@phosphor-icons/react";
import { type AgentKind, type FileChange, type Message } from "~/api";
import type { CanvasTab } from "~/features/canvas/Canvas";
import { FileCards } from "~/features/chat/FileCard";
import { ACTION_CLASS, CopyButton } from "~/features/chat/markdown/CopyButton";
import { Approvals } from "~/features/chat/transcript/components/Approvals";
import { ChangeSummary } from "~/features/chat/transcript/components/ChangeSummary";
import ResponseTimeline from "~/features/chat/response/ResponseTimeline";
import { isCompactCommand, isGoalClearCommand } from "~/lib/composerActions";
import { useReplyFiles } from "~/features/chat/hooks/useReplyFiles";
import { formatClock } from "~/lib/util";

// Markdown loads with the chat, not on first use: its plain-text placeholder had another height,
// so swapping it in moved the messages.

export type OpenCanvas = (path: string, message: Message | null, tab: CanvasTab) => void;

interface AssistantProps {
  message: Message;
  agent: AgentKind;
  projectId: string | null;
  onOpenThread: (id: string) => void;
  changes: FileChange[];
  retryMessage: Message | null;
  busy: boolean;
  onOpenCanvas?: OpenCanvas;
  onShowChanges: (messageId: string) => void;
  onRetry: (message: Message) => void;
  onFork: (message: Message) => void;
  /** The reply came in while its chat was open. */
  arrived?: boolean;
}

/**
 * One reply: a message whose role is `assistant`, whichever agent produced it.
 *
 * "Assistant" is the provider APIs' name for that role, and the counterpart of `UserMessage`; the
 * agent that answered is the thread's, so this draws a Claude, Codex, OpenCode or API reply alike.
 */
export const AssistantMessage = memo(function AssistantMessage({
  message,
  agent,
  projectId,
  onOpenThread,
  changes,
  retryMessage,
  busy,
  onOpenCanvas,
  onShowChanges,
  onRetry,
  onFork,
  arrived = false,
}: AssistantProps) {
  const streaming = message.status === "streaming";
  const clearsGoal = retryMessage !== null && isGoalClearCommand(retryMessage.text);
  // The compacted divider above already says how a /compact is going.
  const compacts = retryMessage !== null && isCompactCommand(retryMessage.text);
  const replyFiles = useReplyFiles(projectId, message.turn, message.files, changes);
  // A file with nothing left to diff opens on its contents instead.
  const openFile =
    onOpenCanvas &&
    ((path: string) =>
      onOpenCanvas(
        path,
        message,
        // A reply with snapshots keeps its turn's diff, even once committed.
        message.turn || changes.some((c) => c.path === path) ? "diff" : "code",
      ));
  return (
    <section className="flex min-w-0 flex-col gap-2.5">
      <ResponseTimeline
        message={message}
        agent={agent}
        clearsGoal={clearsGoal}
        compacts={compacts}
        arrived={arrived}
        onOpenThread={onOpenThread}
        onOpenFile={openFile}
        renderApprovals={(ids) => (
          <Approvals approvals={message.approvals.filter((a) => ids.includes(a.id))} />
        )}
      />
      {/* Replies saved before approvals had their place in the timeline show them here. */}
      <Approvals
        approvals={message.approvals.filter(
          (a) => !message.parts.some((p) => p.type === "approval" && p.id === a.id),
        )}
      />
      {openFile && message.files.length > 0 && (
        <>
          <FileCards
            projectId={projectId}
            changes={changes}
            files={replyFiles}
            onOpen={openFile}
            onShow={() => onShowChanges(message.id)}
          />
          <ChangeSummary
            turn={message.turn}
            paths={message.files}
            changes={changes}
            files={replyFiles}
            onOpen={openFile}
            onShow={() => onShowChanges(message.id)}
          />
        </>
      )}
      {message.refusal && (
        <div
          role="alert"
          className="flex flex-col gap-1 rounded-[var(--radius)] border border-warning bg-[var(--warning-soft)] px-3 py-2 text-[length:var(--fs-sm)] [word-break:break-word]"
        >
          <p className="m-0 flex items-center gap-1.5 font-medium text-warning">
            <ShieldWarningIcon size={15} />
            Blocked by a safety classifier
            {message.refusal.category && ` · ${message.refusal.category}`}
          </p>
          <p className="m-0 whitespace-pre-wrap text-fg">{message.refusal.explanation}</p>
        </div>
      )}
      {message.error && (
        <div
          role="alert"
          className="rounded-[var(--radius)] bg-[var(--danger-soft)] px-3 py-2 text-[length:var(--fs-sm)] whitespace-pre-wrap text-danger [word-break:break-word]"
        >
          {message.error}
        </div>
      )}
      {/* Copy, Retry, Fork and the time belong to a reply; a turn stopped before it wrote one has
          none. Once the text starts the row holds its place, hidden, so it does not push the chat
          up as the reply ends. A cleared goal's status line has nothing to copy or retry. */}
      {!message.cutOff && message.text.trim() && !clearsGoal && (
        <div className={`flex gap-1 ${streaming ? "invisible" : ""}`} aria-hidden={streaming}>
          <CopyButton text={message.text} />
          {retryMessage !== null && (
            <button className={ACTION_CLASS} disabled={busy} onClick={() => onRetry(retryMessage)}>
              <ArrowClockwiseIcon size={13} /> Retry
            </button>
          )}
          <button className={ACTION_CLASS} onClick={() => onFork(message)}>
            <GitForkIcon size={13} /> Fork
          </button>
          <span className="ml-1.5 inline-flex h-6 items-center text-[length:var(--fs-xs)] text-fg-muted">
            {formatClock(message.createdAt)}
          </span>
        </div>
      )}
    </section>
  );
});
