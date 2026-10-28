import { memo } from "react";
import { ArrowClockwiseIcon, ShieldWarningIcon } from "@phosphor-icons/react";
import { type AgentKind, type FileChange, type Message } from "~/api";
import type { CanvasTab } from "~/components/canvas/Canvas";
import { FileCards } from "~/components/chat/FileCard";
import { ACTION_CLASS, CopyButton } from "~/components/chat/CopyButton";
import { Approvals } from "~/components/chat/messages/Approvals";
import { ChangeSummary } from "~/components/chat/messages/ChangeSummary";
import TurnTimeline from "~/components/chat/TurnTimeline";
import { isGoalClearCommand } from "~/components/chat/composerActions";
import { useReplyFiles } from "~/hooks/useReplyFiles";
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
}

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
}: AssistantProps) {
  const streaming = message.status === "streaming";
  const clearsGoal = retryMessage !== null && isGoalClearCommand(retryMessage.text);
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
      <TurnTimeline
        message={message}
        agent={agent}
        clearsGoal={clearsGoal}
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
        <div className="flex flex-col gap-1 rounded-[var(--radius)] border border-warning bg-[var(--warning-soft)] px-3 py-2 text-[length:var(--fs-sm)] [word-break:break-word]">
          <p className="m-0 flex items-center gap-1.5 font-medium text-warning">
            <ShieldWarningIcon size={15} />
            Blocked by a safety classifier
            {message.refusal.category && ` · ${message.refusal.category}`}
          </p>
          <p className="m-0 whitespace-pre-wrap text-fg">{message.refusal.explanation}</p>
        </div>
      )}
      {message.error && (
        <div className="rounded-[var(--radius)] bg-[var(--danger-soft)] px-3 py-2 text-[length:var(--fs-sm)] whitespace-pre-wrap text-danger [word-break:break-word]">
          {message.error}
        </div>
      )}
      {/* Copy, Retry, and the time belong to a reply; a turn stopped before it wrote one has none. */}
      {/* A cleared goal's status line has nothing to copy or retry. */}
      {!streaming && !message.cutOff && message.text.trim() && !clearsGoal && (
        <div className="flex gap-1">
          <CopyButton text={message.text} />
          {retryMessage !== null && (
            <button className={ACTION_CLASS} disabled={busy} onClick={() => onRetry(retryMessage)}>
              <ArrowClockwiseIcon size={13} /> Retry
            </button>
          )}
          <span className="ml-1.5 inline-flex h-6 items-center text-[length:var(--fs-xs)] text-fg-muted">
            {formatClock(message.createdAt)}
          </span>
        </div>
      )}
    </section>
  );
});
