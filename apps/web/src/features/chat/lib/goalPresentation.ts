import type { Goal, GoalStatus } from "~/api";
import { formatTokens } from "~/lib/tokens";
import { fullElapsed } from "~/lib/util";

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

/** A goal that stopped part way and can carry on. A blocked goal has been judged impossible. */
export function goalCanResume(goal: Goal): boolean {
  return (
    goal.status === "paused" || goal.status === "usageLimited" || goal.status === "budgetLimited"
  );
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
  if (seconds >= 60) usage.push(fullElapsed(seconds * 1000));
  if (goal.checks) usage.push(goal.checks === 1 ? "1 check" : `${goal.checks} checks`);
  return usage.length === 0 ? null : usage.join(" · ");
}
