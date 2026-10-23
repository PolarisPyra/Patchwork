import type { Thread } from "~/api";
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
