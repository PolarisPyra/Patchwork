import { useCallback, useLayoutEffect, useMemo, useRef } from "react";
import { LegendList, type LegendListRef } from "@legendapp/list/react";
import { ArrowDownIcon } from "@phosphor-icons/react";
import { api, type FileChange, type Message, type Thread, type TokenSettings } from "~/api";
import { type OpenCanvas } from "~/features/chat/transcript/components/AssistantMessage";
import {
  CompactedDivider,
  TranscriptRow,
  type RowContext,
} from "~/features/chat/transcript/components/TranscriptRow";
import { contextBefore, rowKindOf, visibleMessages } from "~/features/chat/transcript/lib/rows";
import { useTranscriptScroll } from "~/features/chat/transcript/hooks/useTranscriptScroll";
import { shortcutContext, useCommand } from "~/lib/shortcuts";

// Markdown loads with the chat, not on first use: its plain-text placeholder had another height,
// so swapping it in moved the messages.

export interface TranscriptProps {
  thread: Thread;
  /** Opens another thread, such as a subagent's. */
  onOpenThread: (id: string) => void;
  changes: FileChange[];
  busy: boolean;
  /** Omitted for chats without a project: the change summary is hidden. */
  onOpenCanvas?: OpenCanvas;
  /** Opens the sidebar's Changes view. */
  onShowChanges: (messageId: string) => void;
  onRetry: (message: Message) => void;
  /** User messages whose turn can be undone, and undoing one. */
  checkpoints: ReadonlySet<string>;
  onRestore: (message: Message) => void;
  /** Starts a new thread from a reply. */
  onFork: (message: Message) => void;
  tokens: TokenSettings;
}

/**
 * A thread's transcript: its messages in order, and the control that returns to the newest one.
 *
 * The words the chat code uses, in one place, since they are easy to conflate:
 *
 * - **Thread** — one chat, the thing the sidebar lists. Holds its messages; a message never points
 *   back at one.
 * - **Message** — one entry: the user's text, or one reply. It is appended, then rewritten on every
 *   delta while it streams, then finished, retried, rewound or forked by id.
 * - **Turn** — one user message and the reply it got. Both carry the turn's key, so a retry knows
 *   what it is retrying and a checkpoint knows what it would undo.
 * - **Transcript** — the record of a thread's messages, in order. This component draws it, and the
 *   backend pages through it (`getTranscriptPage`) when a chat is longer than one screenful.
 * - **Timeline** — the inside of one reply: its text, reasoning and calls in order, drawn by
 *   `chat/turn/TurnTimeline`.
 *
 * The list is virtual, so a long chat renders only the rows on screen. How a row is drawn lives in
 * `TranscriptRow`, how the list follows its end in `useTranscriptScroll`, and what a row is in
 * `rows`, which leaves this component with the list's wiring and the transcript's own commands.
 */
export default function Transcript({
  thread,
  onOpenThread,
  changes,
  busy,
  onOpenCanvas,
  onShowChanges,
  onRetry,
  checkpoints,
  onRestore,
  onFork,
  tokens,
}: TranscriptProps) {
  // Queued messages wait above the composer until the turn ends, so they are not shown here.
  const messages = useMemo(() => visibleMessages(thread.messages), [thread.messages]);
  // One pass over the thread, so a compacted divider reads the size before it in constant time.
  const before = useMemo(() => contextBefore(messages), [messages]);
  const listRef = useRef<LegendListRef>(null);
  const { atEnd, settled } = useTranscriptScroll(listRef, messages);

  // The list outlives the chat it shows, so its own initial jump to the end happens once, for the
  // first chat. Every chat opened after it is jumped to its end here. Two frames: the rows of the
  // new chat are placed by estimated height first, and only then measured, which moves the end.
  useLayoutEffect(() => {
    let measured = 0;
    const placed = requestAnimationFrame(() => {
      measured = requestAnimationFrame(() => void listRef.current?.scrollToEnd());
    });
    return () => {
      cancelAnimationFrame(placed);
      cancelAnimationFrame(measured);
    };
  }, [thread.id]);

  // A mention of a project file is relative to its root; one of a file outside it is absolute.
  const projectId = thread.projectId;
  const onRevealMention = useCallback(
    (path: string) => void api.revealFile(projectId, path).catch(() => {}),
    [projectId],
  );

  // Rows re-render when these change, since the list keeps them memoized.
  const rowData = useMemo(() => ({ tokens, checkpoints, busy }), [tokens, checkpoints, busy]);
  // The row props that do not vary by message, built once so a row is a diff of one object.
  const rowContext = useMemo<RowContext>(
    () => ({
      thread,
      changes,
      busy,
      tokens,
      checkpoints,
      onOpenThread,
      onOpenCanvas,
      onShowChanges,
      onRetry,
      onRestore,
      onFork,
      onRevealMention,
    }),
    [
      thread,
      changes,
      busy,
      tokens,
      checkpoints,
      onOpenThread,
      onOpenCanvas,
      onShowChanges,
      onRetry,
      onRestore,
      onFork,
      onRevealMention,
    ],
  );

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
          const kind = rowKindOf(item);
          if (kind === "absent") return <div />;
          if (kind === "divider") {
            return <CompactedDivider before={tokens.showUsage ? before[index] : undefined} />;
          }
          return (
            <TranscriptRow
              message={item}
              previous={messages[index - 1]}
              first={index === 0}
              context={rowContext}
            />
          );
        }}
      />
      {!atEnd && (
        <button
          type="button"
          className="timeline-status-enter absolute bottom-3 left-1/2 grid h-8 w-8 -translate-x-1/2 cursor-pointer place-items-center rounded-full border border-border bg-panel text-fg shadow-[var(--shadow-popup)] hover:bg-hover"
          aria-label="Jump to latest message"
          onClick={() => void listRef.current?.scrollToEnd({ animated: true })}
        >
          <ArrowDownIcon size={14} />
        </button>
      )}
    </div>
  );
}
