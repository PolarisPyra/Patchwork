import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { LegendList, type LegendListRef } from "@legendapp/list/react";
import { api } from "~/api";
import { ArrowDownIcon } from "@phosphor-icons/react";
import { type FileChange, type Message, type Thread, type TokenSettings } from "~/api";
import { AssistantMessage, type OpenCanvas } from "~/components/chat/messages/AssistantMessage";
import { UserMessage } from "~/components/chat/messages/UserMessage";
import { isCompactCommand, isGoalClearCommand } from "~/components/chat/composerActions";
import { shortcutContext, useCommand } from "~/lib/shortcuts";
import { formatTokens } from "~/lib/tokens";

// Markdown loads with the chat, not on first use: its plain-text placeholder had another height,
// so swapping it in moved the messages.

interface MessagesProps {
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

  // A mention of a project file is relative to its root; one of a file outside it is absolute.
  const projectId = thread.projectId;
  const revealMention = useCallback(
    (path: string) => void api.revealFile(projectId, path).catch(() => {}),
    [projectId],
  );

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
                    onReveal={revealMention}
                  />
                ) : (
                  <AssistantMessage
                    message={item}
                    agent={thread.agent}
                    projectId={thread.projectId}
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
