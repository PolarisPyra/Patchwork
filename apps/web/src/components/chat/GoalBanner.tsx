import { useEffect, useState } from "react";
import { TargetIcon } from "@phosphor-icons/react";
import type { Goal } from "~/api";
import { goalCanResume, goalLabel, goalUsage } from "~/components/chat/goalPresentation";

const ACTION =
  "flex-none cursor-pointer rounded-[var(--radius)] border-none bg-transparent px-2 py-0.5 text-[length:var(--fs-sm)] text-fg-muted hover:bg-hover hover:text-fg";

/**
 * The goal, on the composer's top edge: its state and usage, then the objective. Stop shows
 * while the agent is pursuing the goal; Resume and Clear show once it is not working on it.
 */
export default function GoalBanner({
  goal,
  working,
  onStop,
  onResume,
  onClear,
}: {
  goal: Goal;
  working: boolean;
  onStop: () => void;
  onResume: () => void;
  onClear: () => void;
}) {
  const active = goal.status === "active";
  const [now, setNow] = useState(() => Date.now());
  // Only an active goal's elapsed time moves; the others show the time they stopped at.
  useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [active]);

  const seconds = goal.timeUsedSeconds ?? Math.max(0, (now - goal.startedAt) / 1000);
  const usage = goalUsage(goal, seconds);
  return (
    <div
      className="mx-3 flex min-w-0 items-center gap-2 px-3 py-1.5 text-[length:var(--fs-sm)]"
      title={goal.objective}
    >
      <TargetIcon
        size={14}
        className={`flex-none ${active ? "text-accent-text" : "text-fg-subtle"}`}
      />
      <span className="flex-none whitespace-nowrap">
        <span className="font-semibold text-fg">{goalLabel(goal, working)}</span>
        {usage && <span className="text-fg-subtle tabular-nums"> · {usage}</span>}
      </span>
      <span className="min-w-0 flex-1 truncate text-fg-muted">{goal.objective}</span>
      {working && active && (
        <button type="button" className={ACTION} onClick={onStop}>
          Stop
        </button>
      )}
      {!working && goalCanResume(goal) && (
        <button type="button" className={ACTION} onClick={onResume}>
          Resume
        </button>
      )}
      {!working && (
        <button type="button" className={ACTION} onClick={onClear}>
          Clear
        </button>
      )}
    </div>
  );
}
