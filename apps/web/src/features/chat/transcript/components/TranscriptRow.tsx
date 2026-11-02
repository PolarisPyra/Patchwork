import { type FileChange, type Message, type Thread, type TokenSettings } from "~/api";
import {
  AssistantMessage,
  type OpenCanvas,
} from "~/features/chat/transcript/components/AssistantMessage";
import { UserMessage } from "~/features/chat/transcript/components/UserMessage";
import Spinner from "~/components/Spinner";
import { type CompactionState } from "~/features/chat/transcript/rows";
import { formatTokens } from "~/lib/tokens";

/**
 * One row of the transcript, and the line that marks a compaction.
 *
 * The row is the only thing that knows how much padding a first, middle or last message gets, so
 * the list above it stays a list and nothing else.
 */

/** The props a row needs that belong to the thread rather than the message. */
export interface RowContext {
  thread: Thread;
  changes: FileChange[];
  busy: boolean;
  tokens: TokenSettings;
  checkpoints: ReadonlySet<string>;
  onOpenThread: (id: string) => void;
  onOpenCanvas?: OpenCanvas;
  onShowChanges: (messageId: string) => void;
  onRetry: (message: Message) => void;
  onRestore: (message: Message) => void;
  onFork: (message: Message) => void;
  /** Opens a project file mentioned in a user's message. */
  onRevealMention: (path: string) => void;
  /** The messages in the chat when it was opened. */
  openedWith: ReadonlySet<string>;
}

/** A message, framed for the transcript. */
export function TranscriptRow({
  message,
  previous,
  first,
  context,
}: {
  message: Message;
  /** The message above, which an edit's retry line refers back to. */
  previous: Message | undefined;
  /** This is the first row of the thread, which gets room above it. */
  first: boolean;
  /** Props shared by every row. */
  context: RowContext;
}) {
  const { thread, changes, busy, checkpoints, onRestore, openedWith, ...handlers } = context;
  const user = message.role === "user";
  return (
    <div className="px-6" role="article" aria-label={user ? "Your message" : "Reply"}>
      <div className={`flex flex-col pb-5${first ? " pt-6" : ""}${user ? "" : " max-w-[820px]"}`}>
        {user ? (
          <UserMessage
            message={message}
            onRestore={!busy && checkpoints.has(message.id) ? onRestore : undefined}
            onReveal={handlers.onRevealMention}
          />
        ) : (
          <AssistantMessage
            message={message}
            threadId={thread.id}
            agent={thread.agent}
            projectId={thread.projectId}
            onOpenThread={handlers.onOpenThread}
            changes={changes}
            retryMessage={previous?.role === "user" ? previous : null}
            busy={busy}
            onOpenCanvas={handlers.onOpenCanvas}
            onShowChanges={handlers.onShowChanges}
            onRetry={handlers.onRetry}
            onFork={handlers.onFork}
            arrived={!openedWith.has(message.id)}
          />
        )}
      </div>
    </div>
  );
}

/**
 * The line a thread gets where its context was compacted: while it runs, then with the size it
 * held before when usage is shown.
 */
export function CompactedDivider({
  state,
  before,
}: {
  state: CompactionState;
  before: number | undefined;
}) {
  return (
    <div className="px-6">
      <div className="flex pb-5">
        <div className="flex w-full items-center gap-3 text-[length:var(--fs-xs)] text-fg-subtle">
          <span className="h-px flex-1 bg-border" />
          {state === "compacting" ? (
            <span className="inline-flex items-center gap-1.5">
              <Spinner size={11} />
              Compacting context
            </span>
          ) : state === "stopped" ? (
            <span>Compaction stopped</span>
          ) : (
            <span>
              Context compacted{before !== undefined && ` · was ${formatTokens(before)} tokens`}
            </span>
          )}
          <span className="h-px flex-1 bg-border" />
        </div>
      </div>
    </div>
  );
}
