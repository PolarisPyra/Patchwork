import { type Message } from "~/api";
import { isCompactCommand, isGoalClearCommand } from "~/lib/composerActions";

/**
 * What a row of the transcript is. Pure, so the rules for a row can be read — and tested — without
 * the virtual list around them.
 */

/** How one message of a thread is drawn. */
export type RowKind =
  /** The user's text, with the time it was sent and whether its turn can be restored. */
  | "user"
  /** A reply: its timeline, files and actions. */
  | "assistant"
  /** A line that says the context was compacted at this point in the thread. */
  | "divider"
  /** Not drawn at all: the reply below already reports it. */
  | "absent";

/** How to draw a message. */
export function rowKindOf(message: Message): RowKind {
  if (message.role !== "user") return "assistant";
  // The green notice in the reply already says the goal was cleared.
  if (isGoalClearCommand(message.text)) return "absent";
  if (isCompactCommand(message.text)) return "divider";
  return "user";
}

/** Where a /compact is: running, done, or stopped before it was. */
export type CompactionState = "compacting" | "compacted" | "stopped";

/** The state of the /compact at `index`, read from the reply after it. */
export function compactionState(
  messages: readonly Message[],
  index: number,
  busy: boolean,
): CompactionState {
  const reply = messages[index + 1];
  // A /compact sent during a turn compacts where the turn is, and the turn's reply streams on.
  if (reply && !reply.compacting && reply.parts.some((p) => p.type === "compaction")) {
    return "compacted";
  }
  if (reply ? reply.status === "streaming" : busy) return "compacting";
  if (!reply || reply.status === "error" || reply.status === "cancelled") return "stopped";
  return "compacted";
}

/** The messages a thread shows: one still queued is waiting above the composer, not a row here. */
export function visibleMessages(messages: readonly Message[]): Message[] {
  return messages.filter((message) => !message.queued);
}

/**
 * The context size in force just before each row, by index, for a compacted divider to compare
 * against. One pass, so drawing a thread is not one backwards scan per row.
 */
export function contextBefore(messages: readonly Message[]): (number | undefined)[] {
  const out: (number | undefined)[] = [];
  let latest: number | undefined;
  for (const message of messages) {
    out.push(latest);
    const context = message.usage?.context;
    if (context !== undefined) latest = context;
  }
  return out;
}

/** The ids that start and answer a freshly sent turn, which the list jumps to. */
export interface Tail {
  /** The newest message the user sent, if any. */
  sent: string | undefined;
  /** The reply that landed directly under it, when the reply has started. */
  reply: string | undefined;
}

/**
 * The end of a thread, as the list follows it. A reply's first row can land just after the sent
 * message and is taller than the list's follow threshold, so it is jumped to as well.
 */
export function tailOf(messages: readonly Message[]): Tail {
  const sent = [...messages].reverse().find((message) => message.role === "user")?.id;
  const reply = messages.at(-2)?.id === sent ? messages.at(-1)?.id : undefined;
  return { sent, reply };
}

/** One turn as the turn rail shows it: where it starts in the list, and its text for a preview. */
export interface Turn {
  /** The id of the user message that starts it. */
  id: string;
  /** That message's row in the list. */
  index: number;
  ask: string;
  /** The text of the last reply before the next turn, if one has come. */
  answer: string | undefined;
}

/** The turns of a thread, one per user message the transcript draws. */
export function turnsOf(messages: readonly Message[]): Turn[] {
  const turns: Turn[] = [];
  messages.forEach((message, index) => {
    if (rowKindOf(message) === "user") {
      turns.push({ id: message.id, index, ask: message.text, answer: undefined });
    } else if (message.role !== "user" && message.text) {
      const turn = turns.at(-1);
      if (turn) turn.answer = message.text;
    }
  });
  return turns;
}
