import { type Thread, type ToolCall } from "~/api";

/**
 * What a call is doing, for the places that draw one: the lineage card beside a chat and the
 * subagent rows in a turn's timeline. Pure and React-free, so the rules are readable on their own.
 */

/** How a call's state reads, and the colour of the dot beside its avatar. */
export const STATUS: Record<ToolCall["status"], { dot: string; label: string }> = {
  pending: { dot: "bg-fg-subtle", label: "Queued" },
  running: { dot: "bg-accent", label: "Running" },
  done: { dot: "bg-success", label: "Completed" },
  error: { dot: "bg-danger", label: "Failed" },
};

/** Tool calls of the current turn: every assistant message since the last non-steer user message. */
export function currentTurnTools(thread: Thread): ToolCall[] {
  let start = -1;
  thread.messages.forEach((m, i) => {
    if (m.role === "user" && !m.steer) start = i;
  });
  return thread.messages
    .slice(start + 1)
    .filter((m) => m.role === "assistant")
    .flatMap((m) => m.tools);
}

/** Running or about to run. Finished calls are "done" or "error". */
export function isActive(tool: ToolCall): boolean {
  return tool.status === "running" || tool.status === "pending";
}

/** What a call is called in a row: a subagent's own description, else its kind, else the tool's. */
export function titleOf(tool: ToolCall): string {
  return tool.subagent?.description || tool.subagent?.kind || tool.name;
}

/** Time a call has run, or null when its start is unknown. */
export function elapsedMs(tool: ToolCall, now: number): number | null {
  if (tool.startedAt === null) return null;
  return Math.max(0, (tool.endedAt ?? now) - tool.startedAt);
}
