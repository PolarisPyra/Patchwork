import { useEffect, useState } from "react";
import { api, type Thread } from "~/api";
import { errorText } from "~/lib/util";

/** A goal clear in the banner: pending while its error is null, failed otherwise. */
interface GoalClear {
  key: string;
  error: string | null;
}

/**
 * Clearing the goal a chat is pursuing. The clear is keyed by the goal, so a later goal never
 * shows an earlier one's state, and it goes back to being offered if the agent never confirms it.
 */
export function useGoalClear(thread: Thread | null, working: boolean) {
  const [goalClear, setGoalClear] = useState<GoalClear | null>(null);

  // A clear still pending once its run has ended was not confirmed by the agent, so the banner
  // goes back to offering it.
  useEffect(() => {
    if (!working) setGoalClear((clear) => (clear?.error === null ? null : clear));
  }, [working]);

  const goalKey = thread?.goal ? `${thread.id}:${thread.goal.startedAt}` : null;
  const clearOfGoal = goalClear?.key === goalKey ? goalClear : null;

  /** Clears the goal, in the agent's session too. The banner offers it once no turn is running. */
  function clearGoal() {
    if (!thread?.goal) return;
    const key = `${thread.id}:${thread.goal.startedAt}`;
    setGoalClear({ key, error: null });
    api.stopGoal(thread.id).catch((e: unknown) => setGoalClear({ key, error: errorText(e) }));
  }

  return { clearOfGoal, clearGoal };
}
