import type { Goal, GoalStatus } from "~/api";

export const GOAL_LABELS: Record<GoalStatus, string> = {
  active: "Pursuing goal",
  paused: "Goal paused",
  blocked: "Goal blocked",
  usageLimited: "Goal stopped at usage limit",
  budgetLimited: "Goal reached token budget",
  complete: "Goal complete",
};

/** The banner's title. An active goal the agent is not working on right now is only set. */
export function goalLabel(goal: Goal, working: boolean): string {
  return goal.status === "active" && !working ? "Goal set" : GOAL_LABELS[goal.status];
}

/** The agent's reply to `/goal clear`, which reports the goal cleared or none to clear. */
export function isGoalClearedReply(text: string): boolean {
  return /^(Goal cleared|No goal set|No goal to clear)/.test(text.trim());
}

/** `/goal clear` or one of its aliases, as the backend reads them. */
export function isGoalClearCommand(text: string): boolean {
  return /^\/goal\s+(clear|stop|off|reset|none|cancel)\s*$/i.test(text.trim());
}

/** A goal that stopped part way and can carry on. A blocked goal has been judged impossible. */
export function goalCanResume(goal: Goal): boolean {
  return (
    goal.status === "paused" || goal.status === "usageLimited" || goal.status === "budgetLimited"
  );
}

/** "45s", "3m 12s", "1h 4m". */
function formatDuration(seconds: number): string {
  const s = Math.floor(seconds);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${s % 60}s`;
  return `${Math.floor(m / 60)}h ${m % 60}m`;
}

/** "950", "19k", "1.5m". */
function formatTokens(n: number): string {
  if (n < 1_000) return `${n}`;
  if (n < 1_000_000) return `${Math.round(n / 1_000)}k`;
  return `${(n / 1_000_000).toFixed(1).replace(/\.0$/, "")}m`;
}

/** The tokens, time, and checks of a goal, joined for the banner. Time shows from a minute. */
export function goalUsage(goal: Goal, seconds: number): string | null {
  const usage: string[] = [];
  if (goal.tokensUsed != null && goal.tokensUsed > 0) {
    usage.push(
      goal.tokenBudget == null
        ? `${formatTokens(goal.tokensUsed)} tokens`
        : `${formatTokens(goal.tokensUsed)} / ${formatTokens(goal.tokenBudget)} tokens`,
    );
  }
  if (seconds >= 60) usage.push(formatDuration(seconds));
  if (goal.checks) usage.push(goal.checks === 1 ? "1 check" : `${goal.checks} checks`);
  return usage.length === 0 ? null : usage.join(" · ");
}
