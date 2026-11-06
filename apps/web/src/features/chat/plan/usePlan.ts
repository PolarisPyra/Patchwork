import { useEffect, useRef, useState } from "react";
import type { PlanStep, Thread } from "~/api";
import { planTitle } from "~/features/chat/plan/ProposedPlanCard";
import type { NewThreadOptions } from "~/hooks/useThreads";
import { errorText } from "~/lib/util";

const IMPLEMENT = "PLEASE IMPLEMENT THIS PLAN:\n";

/** How long a finished list stays before it folds away. */
const FOLD_MS = 1200;

/** The reply whose plan steps the strip shows, and the steps it held when they folded away. */
interface ShownPlan {
  id: string;
  folded: string | null;
}

const stepsKey = (steps: PlanStep[]) =>
  steps.map((step) => `${step.status} ${step.text}`).join("\n");

/**
 * A thread's plans: the steps of the reply running now, and a plan the last reply proposed in plan
 * mode, ready to implement or refine.
 */
export function usePlan({
  thread,
  working,
  asking,
  send,
  onStartThread,
  onSetPlanMode,
  setError,
}: {
  thread: Thread | null;
  working: boolean;
  /** The agent is waiting on an answer, so its plan is not ready yet. */
  asking: boolean;
  send: (text: string) => Promise<unknown>;
  onStartThread: (opts: NewThreadOptions, title: string, text: string) => Promise<void>;
  onSetPlanMode: (threadId: string, on: boolean) => Promise<void>;
  setError: (text: string | null) => void;
}) {
  // The plan of the reply running now. Once that reply ends, finished or not, its plan stays a
  // moment and then folds away, so nothing looks under way when nothing is. Plans of replies that
  // ended before the chat was opened are not shown. Queued messages wait after the reply, so they
  // are skipped.
  const lastMessage = thread?.messages.filter((m) => !m.queued).at(-1);
  const livePlanId =
    working &&
    lastMessage?.role === "assistant" &&
    lastMessage.status === "streaming" &&
    lastMessage.plan.length > 0
      ? lastMessage.id
      : null;
  const [shown, setShown] = useState<ShownPlan | null>(null);
  useEffect(() => {
    if (livePlanId !== null) {
      setShown((s) => (s?.id === livePlanId ? s : { id: livePlanId, folded: null }));
    }
  }, [livePlanId]);
  const planId =
    lastMessage?.role === "assistant" &&
    lastMessage.plan.length > 0 &&
    (lastMessage.id === livePlanId || lastMessage.id === shown?.id)
      ? lastMessage.id
      : null;
  const steps = planId !== null && lastMessage ? lastMessage.plan : [];
  const key = stepsKey(steps);
  // A finished list, or one whose reply ended, stays a moment, then folds away; a new list brings
  // it back.
  const over = steps.length > 0 && (livePlanId === null || steps.every((s) => s.status === "done"));
  const folded = over && shown?.id === planId && shown.folded === key;
  useEffect(() => {
    if (!over || planId === null) return;
    const timer = window.setTimeout(
      () => setShown((s) => (s?.id === planId ? { ...s, folded: key } : s)),
      FOLD_MS,
    );
    return () => window.clearTimeout(timer);
  }, [over, planId, key]);

  const proposed =
    thread?.planMode && !working && !asking && lastMessage?.role === "assistant"
      ? [...lastMessage.parts].reverse().find((p) => p.type === "plan")
      : undefined;
  const readyPlan = proposed?.type === "plan" ? proposed.text : null;
  // Threads whose plan is being implemented, so a second click or Enter does not send it again.
  const implementing = useRef(new Set<string>());
  // Implementing leaves plan mode, in this thread or a new one named after the plan.
  async function implementPlan(newThread: boolean) {
    if (!thread || readyPlan === null || implementing.current.has(thread.id)) return;
    implementing.current.add(thread.id);
    const text = IMPLEMENT + readyPlan.trim();
    setError(null);
    try {
      if (newThread) {
        const opts: NewThreadOptions = {
          agent: thread.agent,
          model: thread.model,
          accessMode: thread.accessMode,
          effort: thread.effort,
          compactWindow: thread.compactWindow,
        };
        await onStartThread(opts, `Implement ${planTitle(readyPlan)}`.slice(0, 60), text);
      } else {
        await onSetPlanMode(thread.id, false);
        await send(text);
      }
    } catch (e) {
      setError(errorText(e));
    } finally {
      implementing.current.delete(thread.id);
    }
  }

  return {
    /** The running reply's steps, kept while they fold away. */
    steps,
    /** The steps show in the strip. */
    stepsShown: steps.length > 0 && !folded,
    readyPlan,
    implementPlan,
  };
}
