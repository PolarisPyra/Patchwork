import { lazy, memo, Suspense, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { LegendList, type LegendListRef } from "@legendapp/list/react";
import {
  ArrowClockwiseIcon,
  ArrowDownIcon,
  CheckCircleIcon,
  GitDiffIcon,
  ShieldWarningIcon,
  WarningIcon,
  XCircleIcon,
} from "@phosphor-icons/react";
import {
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
import { formatTokens } from "~/lib/tokens";
import { formatClock } from "~/lib/util";

// Markdown pulls in the renderer, so it loads on first use; plain text shows until then.
const Markdown = lazy(() => import("~/components/chat/Markdown"));

type OpenCanvas = (path: string, message: Message | null, tab: CanvasTab) => void;

interface MessagesProps {
  thread: Thread;
  changes: FileChange[];
  busy: boolean;
  /** Omitted for chats without a project: the change summary is hidden. */
  onOpenCanvas?: OpenCanvas;
  /** Opens the sidebar's Changes view. */
  onShowChanges: (paths: string[]) => void;
  onRetry: (message: Message) => void;
  tokens: TokenSettings;
}

export default function Messages({
  thread,
  changes,
  busy,
  onOpenCanvas,
  onShowChanges,
  onRetry,
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

  // Start at the latest message and jump to it again after each send. While a reply streams, the
  // list keeps following the end as long as the reader is at the bottom.
  const lastUserMessageId = [...messages].reverse().find((m) => m.role === "user")?.id;
  const lastSent = useRef(lastUserMessageId);
  useLayoutEffect(() => {
    if (lastSent.current === lastUserMessageId) return; // mount: initialScrollAtEnd handles it
    lastSent.current = lastUserMessageId;
    void listRef.current?.scrollToEnd();
  }, [lastUserMessageId]);

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      <LegendList
        ref={listRef}
        data={messages}
        keyExtractor={(row) => row.id}
        extraData={tokens}
        getItemType={(row) => row.role}
        estimatedItemSize={90}
        initialScrollAtEnd
        maintainScrollAtEnd
        maintainVisibleContentPosition
        className="min-h-0 flex-1 overflow-x-hidden [overflow-anchor:none]"
        renderItem={({ item, index }) => {
          if (item.role === "user" && item.text.trim() === "/compact") {
            const before = [...messages.slice(0, index)].reverse().find((m) => m.usage)?.usage;
            return (
              <div className="px-6">
                <div className="flex pb-5">
                  <CompactedDivider before={tokens.showUsage ? before?.context : undefined} />
                </div>
              </div>
            );
          }
          const previous = messages[index - 1];
          const usage =
            item.role === "assistant" && item.status !== "streaming" && tokens.showUsage;
          return (
            <div className="px-6">
              <div
                className={`flex flex-col pb-5${index === 0 ? " pt-6" : ""}${item.role === "user" ? "" : " max-w-[820px]"}`}
              >
                {item.role === "user" ? (
                  <UserMessage message={item} />
                ) : (
                  <AssistantMessage
                    message={item}
                    changes={changes}
                    retryMessage={previous?.role === "user" ? previous : null}
                    busy={busy}
                    onOpenCanvas={onOpenCanvas}
                    onShowChanges={onShowChanges}
                    onRetry={onRetry}
                  />
                )}
              </div>
              {usage && item.usage && (
                // Pulled up into the message's bottom padding, and out to the column's right edge.
                <div className="-mt-2 mb-1 flex justify-end">
                  <UsageLine usage={item.usage} warnAbove={tokens.warnAbove} />
                </div>
              )}
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

const UserMessage = memo(function UserMessage({ message }: { message: Message }) {
  return (
    <div className="flex max-w-[75%] flex-col items-end gap-1 self-end">
      <div className="px-2 text-[length:var(--fs-xs)] text-fg-subtle">
        You · {formatClock(message.createdAt)}
        {message.steer && " · Steer"}
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
          <Suspense fallback={<div className="whitespace-pre-wrap">{message.text}</div>}>
            <Markdown text={message.text} />
          </Suspense>
        </div>
      )}
    </div>
  );
});

interface AssistantProps {
  message: Message;
  changes: FileChange[];
  retryMessage: Message | null;
  busy: boolean;
  onOpenCanvas?: OpenCanvas;
  onShowChanges: (paths: string[]) => void;
  onRetry: (message: Message) => void;
}

const AssistantMessage = memo(function AssistantMessage({
  message,
  changes,
  retryMessage,
  busy,
  onOpenCanvas,
  onShowChanges,
  onRetry,
}: AssistantProps) {
  const streaming = message.status === "streaming";
  return (
    <section className="flex min-w-0 flex-col gap-2.5">
      <TurnTimeline message={message} />
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
      {message.error && (
        <div className="rounded-[var(--radius)] bg-[var(--danger-soft)] px-3 py-2 text-[length:var(--fs-sm)] whitespace-pre-wrap text-danger [word-break:break-word]">
          {message.error}
        </div>
      )}
      {/* Copy, Retry, and the time belong to a reply; a turn stopped before it wrote one has none. */}
      {!streaming && message.text.trim() && (
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

/** The tokens a reply used. A context over `warnAbove` is flagged, since every turn re-reads it. */
function UsageLine({ usage, warnAbove }: { usage: TokenUsage; warnAbove: number }) {
  const large = usage.context > warnAbove;
  return (
    <p
      className={`m-0 flex flex-wrap items-center justify-end gap-x-3 px-1 text-[length:var(--fs-xs)] tabular-nums ${large ? "text-warning" : "text-fg-muted"}`}
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
