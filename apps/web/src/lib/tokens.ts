import type { Message, Thread, TokenUsage } from "~/api";
import { isCompactCommand } from "~/components/chat/composerActions";

/** Claude's prompt cache lasts an hour; a thread idle longer has lost it. */
export const COLD_AFTER_MS = 60 * 60 * 1000;

/** Token count for display: "842", "12.4k", "180k", "1.2m". */
export function formatTokens(n: number): string {
  if (n < 1000) return String(n);
  if (n < 100_000) return `${(n / 1000).toFixed(1).replace(/\.0$/, "")}k`;
  if (n < 1_000_000) return `${Math.round(n / 1000)}k`;
  return `${(n / 1_000_000).toFixed(1).replace(/\.0$/, "")}m`;
}

/**
 * A thread's tokens: the context of its latest reply, and what all its replies used together.
 * Null when no reply has reported usage.
 */
export function threadUsage(messages: Message[]): TokenUsage | null {
  const last = [...messages].reverse().find((m) => m.usage)?.usage;
  if (!last) return null;
  const total: TokenUsage = {
    input: 0,
    cacheRead: 0,
    cacheWrite: 0,
    output: 0,
    context: last.context,
  };
  for (const { usage } of messages) {
    if (!usage) continue;
    total.input += usage.input;
    total.cacheRead += usage.cacheRead;
    total.cacheWrite += usage.cacheWrite;
    total.output += usage.output;
  }
  return total;
}

/**
 * A Claude thread whose context is over `warnAbove` and whose prompt cache has expired, so
 * resuming it would re-read all of that context at full price. Null when it is not, or when
 * its last reply was a `/compact`, whose usage still reports the context from before it ran.
 */
export function coldLargeSession(
  thread: Thread,
  now: number,
  warnAbove: number,
): { idleMs: number; context: number } | null {
  if (thread.agent !== "claude") return null;
  const last = thread.messages.at(-1);
  const lastUser = [...thread.messages].reverse().find((m) => m.role === "user");
  if (!last?.usage || last.finishedAt === null || isCompactCommand(lastUser?.text ?? "")) {
    return null;
  }
  const idleMs = now - last.finishedAt;
  if (idleMs < COLD_AFTER_MS || last.usage.context <= warnAbove) return null;
  return { idleMs, context: last.usage.context };
}
