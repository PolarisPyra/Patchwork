import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { LegendList, type LegendListRef } from "@legendapp/list/react";
import {
  ArrowClockwiseIcon,
  ArrowCounterClockwiseIcon,
  ArrowDownIcon,
  CheckCircleIcon,
  GitDiffIcon,
  ShieldWarningIcon,
  WarningIcon,
  XCircleIcon,
} from "@phosphor-icons/react";
import {
  type AgentKind,
  type Approval,
  type FileChange,
  type Message,
  type Thread,
  type TokenSettings,
  type TokenUsage,
} from "~/api";
import type { CanvasTab } from "~/components/canvas/Canvas";
import AttachmentChip, { AttachmentTile } from "~/components/chat/AttachmentChip";
import { ACTION_CLASS, CopyButton } from "~/components/chat/CopyButton";
import TurnTimeline from "~/components/chat/TurnTimeline";
import {
  goalObjective,
  isCompactCommand,
  isGoalClearCommand,
} from "~/components/chat/composerActions";
import { GOAL_CLASS } from "~/components/chat/composerExtensions";
import { shortcutContext, useCommand } from "~/lib/shortcuts";
import { formatTokens } from "~/lib/tokens";
import { formatClock } from "~/lib/util";

// Markdown loads with the chat, not on first use: its plain-text placeholder had another height,
// so swapping it in moved the messages.
import Markdown from "~/components/chat/Markdown";

type OpenCanvas = (path: string, message: Message | null, tab: CanvasTab) => void;

interface MessagesProps {
  thread: Thread;
  /** Opens another thread, such as a subagent's. */
  onOpenThread: (id: string) => void;
  changes: FileChange[];
  busy: boolean;
  /** Omitted for chats without a project: the change summary is hidden. */
  onOpenCanvas?: OpenCanvas;
  /** Opens the sidebar's Changes view. */
  onShowChanges: (paths: string[]) => void;
  onRetry: (message: Message) => void;
  /** User messages whose turn can be undone, and undoing one. */
  checkpoints: ReadonlySet<string>;
  onRestore: (message: Message) => void;
  tokens: TokenSettings;
}

export default function Messages({
  thread,
  onOpenThread,
  changes,
  busy,
  onOpenCanvas,
  onShowChanges,
  onRetry,
  checkpoints,
  onRestore,
  tokens,
}: MessagesProps) {
  // Queued messages wait above the composer until the turn ends, so they are not shown here.
  const messages = useMemo(() => thread.messages.filter((m) => !m.queued), [thread.messages]);
  const listRef = useRef<LegendListRef>(null);
  const [atEnd, setAtEnd] = useState(true);

  // The same threshold the list uses to keep following the end, so the button only shows when the
  // list will not follow on its own.
  useEffect(
    () => listRef.current?.getState().listen("isWithinMaintainScrollAtEndThreshold", setAtEnd),
    [],
  );

  // The list first places rows by estimated height and only reaches the end a frame or two later,
  // once they are measured. It stays hidden until it has held the end for two frames, so opening a
  // chat does not show it jumping. The frame cap shows it anyway if it never settles.
  const [settled, setSettled] = useState(false);
  useEffect(() => {
    let frame = 0;
    let held = 0;
    let frames = 0;
    const check = () => {
      // Measured rows can move the end after the first placement, so keep jumping to it until held.
      const atEnd = listRef.current?.getState().isAtEnd ?? false;
      if (!atEnd) listRef.current?.scrollToEnd();
      held = atEnd ? held + 1 : 0;
      if (held >= 2 || ++frames > 30) setSettled(true);
      else frame = requestAnimationFrame(check);
    };
    frame = requestAnimationFrame(check);
    return () => cancelAnimationFrame(frame);
  }, []);

  // Start at the latest message and jump to it again after each send. While a reply streams, the
  // list keeps following the end as long as the reader is at the bottom.
  const lastUserMessageId = [...messages].reverse().find((m) => m.role === "user")?.id;
  const lastSent = useRef(lastUserMessageId);
  // The reply's first row can land just after the sent message, and is taller than the list's
  // follow threshold, so the list does not always follow it on its own; it is jumped to as well.
  const replyId = messages.at(-2)?.id === lastUserMessageId ? messages.at(-1)?.id : undefined;
  const lastReply = useRef(replyId);
  useLayoutEffect(() => {
    // mount: initialScrollAtEnd handles it
    if (lastSent.current === lastUserMessageId && lastReply.current === replyId) return;
    lastSent.current = lastUserMessageId;
    lastReply.current = replyId;
    void listRef.current?.scrollToEnd();
  }, [lastUserMessageId, replyId]);

  // Rows re-render when these change, since the list keeps them memoized.
  const rowData = useMemo(() => ({ tokens, checkpoints, busy }), [tokens, checkpoints, busy]);

  // The app disables text selection by default, so the browser's select-all would not reach the
  // transcript. Focus lands here on click, and Ctrl/Cmd+A then selects what the list has rendered.
  const rootRef = useRef<HTMLDivElement>(null);
  useCommand("transcript.selectAll", () => {
    if (rootRef.current) window.getSelection()?.selectAllChildren(rootRef.current);
  });
  useCommand("transcript.clearSelection", () => window.getSelection()?.removeAllRanges());

  return (
    <div
      className="relative flex min-h-0 flex-1 flex-col outline-none"
      ref={rootRef}
      tabIndex={-1}
      {...shortcutContext("transcriptFocus")}
    >
      <LegendList
        ref={listRef}
        data={messages}
        keyExtractor={(row) => row.id}
        extraData={rowData}
        getItemType={(row) => row.role}
        estimatedItemSize={90}
        initialScrollAtEnd
        maintainScrollAtEnd
        maintainVisibleContentPosition
        className={`min-h-0 flex-1 overflow-x-hidden [overflow-anchor:none]${settled ? "" : " invisible"}`}
        renderItem={({ item, index }) => {
          if (item.role === "user" && isCompactCommand(item.text)) {
            const before = [...messages.slice(0, index)].reverse().find((m) => m.usage)?.usage;
            return (
              <div className="px-6">
                <div className="flex pb-5">
                  <CompactedDivider before={tokens.showUsage ? before?.context : undefined} />
                </div>
              </div>
            );
          }
          // The green notice in the reply already says the goal was cleared.
          if (item.role === "user" && isGoalClearCommand(item.text)) return <div />;
          const previous = messages[index - 1];
          return (
            <div className="px-6">
              <div
                className={`flex flex-col pb-5${index === 0 ? " pt-6" : ""}${item.role === "user" ? "" : " max-w-[820px]"}`}
              >
                {item.role === "user" ? (
                  <UserMessage
                    message={item}
                    onRestore={!busy && checkpoints.has(item.id) ? onRestore : undefined}
                  />
                ) : (
                  <AssistantMessage
                    message={item}
                    agent={thread.agent}
                    onOpenThread={onOpenThread}
                    changes={changes}
                    retryMessage={previous?.role === "user" ? previous : null}
                    busy={busy}
                    onOpenCanvas={onOpenCanvas}
                    onShowChanges={onShowChanges}
                    onRetry={onRetry}
                  />
                )}
              </div>
            </div>
          );
        }}
      />
      {!atEnd && (
        <button
          type="button"
          className="absolute bottom-3 left-1/2 grid h-8 w-8 -translate-x-1/2 cursor-pointer place-items-center rounded-full border border-border bg-panel text-fg shadow-[var(--shadow-popup)] hover:bg-hover"
          aria-label="Jump to latest message"
          onClick={() => void listRef.current?.scrollToEnd({ animated: true })}
        >
          <ArrowDownIcon size={14} />
        </button>
      )}
    </div>
  );
}

/** User messages longer than this, such as pasted logs, show their first lines until expanded:
 * laying out all of one stalled the chat for over 100ms each time it scrolled into view. */
const LONG_MESSAGE_CHARS = 20000;
const PREVIEW_LINES = 40;
/** Long messages the user expanded; kept here because the chat list remounts rows as it scrolls. */
const expandedMessages = new Set<string>();

const UserMessage = memo(function UserMessage({
  message,
  onRestore,
}: {
  message: Message;
  /** Set when the message's turn can be undone. */
  onRestore?: (message: Message) => void;
}) {
  const long = message.text.length > LONG_MESSAGE_CHARS;
  const [expanded, setExpanded] = useState(() => expandedMessages.has(message.id));
  const lines = useMemo(() => (long ? message.text.split("\n") : []), [long, message.text]);
  const text =
    long && !expanded && lines.length > PREVIEW_LINES
      ? lines.slice(0, PREVIEW_LINES).join("\n")
      : long && !expanded
        ? message.text.slice(0, LONG_MESSAGE_CHARS)
        : message.text;
  const objective = goalObjective(text);
  const toggle = () => {
    if (expanded) expandedMessages.delete(message.id);
    else expandedMessages.add(message.id);
    setExpanded(!expanded);
  };
  return (
    <div className="group flex max-w-[75%] flex-col items-end gap-1 self-end">
      <div className="flex items-center gap-1 px-2 text-[length:var(--fs-xs)] text-fg-subtle">
        {onRestore && (
          <button
            className={`${ACTION_CLASS} opacity-0 group-hover:opacity-100 focus-visible:opacity-100`}
            title="Put the project's files back as they were before this message"
            onClick={() => onRestore(message)}
          >
            <ArrowCounterClockwiseIcon size={13} /> Restore
          </button>
        )}
        <span>
          You · {formatClock(message.createdAt)}
          {message.steer && " · Steer"}
        </span>
      </div>
      {message.attachments.length > 0 && (
        <div className="flex flex-wrap justify-end gap-1.5">
          {message.attachments.map((a) =>
            a.kind === "image" ? (
              <AttachmentTile key={a.path} attachment={a} />
            ) : (
              <AttachmentChip key={a.path} attachment={a} />
            ),
          )}
        </div>
      )}
      {message.text && (
        // Line breaks the user typed are kept inside paragraphs only; between blocks they would show
        // as blank lines.
        <div className="rounded-[12px] bg-[color-mix(in_srgb,var(--text)_10%,var(--bg-editor))] px-4 py-2.5 leading-[1.55] text-fg [word-break:break-word] selectable [&_h3]:mt-0 [&_h3]:mb-0 [&_p]:whitespace-pre-wrap">
          {objective !== null ? (
            // The same chip the composer shows for `/goal`, then the objective.
            <div className="flex items-baseline gap-1.5">
              <span className={`flex-none ${GOAL_CLASS}`}>/goal</span>
              <div className="min-w-0 flex-1">
                <Markdown text={objective} typed />
              </div>
            </div>
          ) : (
            <Markdown text={text} typed />
          )}
        </div>
      )}
      {long && (
        <button className={ACTION_CLASS} onClick={toggle}>
          {expanded
            ? "Show less"
            : `Show full message (${Math.round(message.text.length / 1000)}k characters)`}
        </button>
      )}
    </div>
  );
});

interface AssistantProps {
  message: Message;
  agent: AgentKind;
  onOpenThread: (id: string) => void;
  changes: FileChange[];
  retryMessage: Message | null;
  busy: boolean;
  onOpenCanvas?: OpenCanvas;
  onShowChanges: (paths: string[]) => void;
  onRetry: (message: Message) => void;
}

const AssistantMessage = memo(function AssistantMessage({
  message,
  agent,
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
  return (
    <section className="flex min-w-0 flex-col gap-2.5">
      <TurnTimeline
        message={message}
        agent={agent}
        clearsGoal={clearsGoal}
        onOpenThread={onOpenThread}
      />
      {/* A pending approval is answered in the banner above the composer. */}
      {message.approvals.map((approval) =>
        approval.status === "pending" ? (
          <p
            key={approval.id}
            className="m-0 flex items-center gap-1.5 px-1 text-[length:var(--fs-sm)] text-warning"
          >
            <ShieldWarningIcon size={15} />
            {approval.questions?.length
              ? "Waiting for your answer"
              : `Waiting for approval: ${approval.title}`}
          </p>
        ) : (
          <ApprovalCard key={approval.id} approval={approval} />
        ),
      )}
      {onOpenCanvas && message.files.length > 0 && (
        <ChangeSummary
          paths={message.files}
          changes={changes}
          onShow={() => onShowChanges(message.files)}
        />
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

/** The thread's tokens, shown above the composer. A context over `warnAbove` is flagged, since every turn re-reads it. */
export function UsageLine({ usage, warnAbove }: { usage: TokenUsage; warnAbove: number }) {
  const large = usage.context > warnAbove;
  return (
    <p
      className={`m-0 mb-1.5 flex flex-wrap items-center justify-end gap-x-3 pr-3 text-[length:var(--fs-xs)] tabular-nums ${large ? "text-warning" : "text-fg-muted"}`}
    >
      <span className="inline-flex items-center gap-1">
        {large && <WarningIcon size={13} />}
        {large
          ? `Context over ${formatTokens(warnAbove)}`
          : `Context ${formatTokens(usage.context)}`}
      </span>
      <span>Cache read {formatTokens(usage.cacheRead)} tokens</span>
      <span>Cache write {formatTokens(usage.cacheWrite)} tokens</span>
      <span>Input {formatTokens(usage.input)} tokens</span>
      <span>Output {formatTokens(usage.output)} tokens</span>
    </p>
  );
}

/** A resolved approval, kept in the transcript with its outcome. */
function CompactedDivider({ before }: { before: number | undefined }) {
  return (
    <div className="flex w-full items-center gap-3 text-[length:var(--fs-xs)] text-fg-subtle">
      <span className="h-px flex-1 bg-border" />
      <span>
        Context compacted{before !== undefined && ` · was ${formatTokens(before)} tokens`}
      </span>
      <span className="h-px flex-1 bg-border" />
    </div>
  );
}

function ApprovalCard({ approval }: { approval: Approval }) {
  const { title, detail, status, questions } = approval;
  if (questions?.length) return <AnsweredQuestions approval={approval} />;
  return (
    <div className="flex flex-col gap-2 rounded-[var(--radius)] border border-border-strong bg-panel px-3 py-2.5 text-[length:var(--fs-sm)]">
      <div
        className={`flex items-center gap-1.5 ${status === "allowed" ? "text-success" : "text-danger"}`}
      >
        {status === "allowed" ? <CheckCircleIcon size={15} /> : <XCircleIcon size={15} />}
        <span className="font-semibold text-fg">{title}</span>
        <span className="ml-auto">{status === "allowed" ? "Allowed" : "Denied"}</span>
      </div>
      {detail && (
        <code className="rounded-[var(--radius-sm)] bg-editor px-2 py-1.5 font-[family-name:var(--font-mono)] break-all whitespace-pre-wrap selectable">
          {detail}
        </code>
      )}
    </div>
  );
}

/** The agent's questions and the user's answers, or that the user declined to answer. */
function AnsweredQuestions({ approval }: { approval: Approval }) {
  const { questions = [], answers = {}, status } = approval;
  return (
    <div className="flex flex-col gap-1.5 rounded-[var(--radius)] border border-border-strong bg-panel px-3 py-2.5 text-[length:var(--fs-sm)]">
      {questions.map((q) => (
        <div key={q.id} className="flex flex-col gap-0.5">
          <span className="font-semibold text-fg selectable">{q.question}</span>
          <span className="text-fg-muted selectable">
            {status === "denied" ? "Not answered" : (answers[q.id] ?? []).join(", ")}
          </span>
        </div>
      ))}
    </div>
  );
}

/** One line for the files a message touched; the list itself lives in the Changes view. */
function ChangeSummary({
  paths,
  changes,
  onShow,
}: {
  paths: string[];
  changes: FileChange[];
  onShow: () => void;
}) {
  const found = changes.filter((c) => paths.includes(c.path));
  const additions = found.reduce((sum, c) => sum + c.additions, 0);
  const deletions = found.reduce((sum, c) => sum + c.deletions, 0);
  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-2 rounded-[var(--radius)] border border-border bg-panel px-3 py-2 text-[length:var(--fs-sm)]">
      <GitDiffIcon size={14} className="flex-none text-fg-muted" />
      <span className="whitespace-nowrap">
        {paths.length} {paths.length === 1 ? "file" : "files"} changed
      </span>
      {found.length > 0 && (
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
  );
}
