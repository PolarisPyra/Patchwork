import { useEffect, useState } from "react";
import { CaretRightIcon, TargetIcon, XIcon } from "@phosphor-icons/react";
import type { Goal } from "~/api";
import { goalCanResume, goalLabel, goalUsage } from "~/components/chat/goalPresentation";
import { ComposerStripBody } from "~/components/chat/ComposerStrip";

const ACTION =
  "inline-flex h-6 flex-none cursor-pointer items-center justify-center rounded-[var(--radius-sm)] border border-border-strong bg-editor px-2.5 text-[length:var(--fs-xs)] text-fg hover:bg-hover";

/**
 * The goal, on the composer's top edge: its state and usage, then the objective. Its last state
 * remains visible until the user closes it.
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
  const [open, setOpen] = useState(true);
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
  const label = goalLabel(goal, working);
  return (
    <div className="composer-strip mx-2 mb-1.5 overflow-hidden rounded-[var(--radius-sm)] border border-border-strong bg-panel text-[length:var(--fs-sm)] shadow-sm">
      <div className="flex h-9 min-w-0 items-center gap-2 bg-app px-2.5">
        <button
          type="button"
          aria-expanded={open}
          onClick={() => setOpen((value) => !value)}
          className="flex min-w-0 flex-1 cursor-pointer items-center gap-2 border-none bg-transparent p-0 text-left text-fg-muted hover:text-fg"
          title={goal.objective}
        >
          <TargetIcon
            size={13}
            className={`composer-strip-tone flex-none ${active ? "text-accent-text" : "text-fg-subtle"}`}
          />
          <span
            key={label}
            className="composer-strip-state flex-none whitespace-nowrap font-semibold text-fg"
          >
            {label}
          </span>
          {usage && (
            <span className="min-w-0 truncate text-[length:var(--fs-xs)] text-fg-subtle tabular-nums">
              {usage}
            </span>
          )}
          <CaretRightIcon
            size={12}
            className="composer-strip-caret ml-auto flex-none"
            data-open={open}
          />
        </button>
        <div
          key={`${goal.status}-${working}`}
          className="composer-strip-state flex flex-none items-center gap-2"
        >
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
            <button
              type="button"
              className="grid h-6 w-6 flex-none cursor-pointer place-items-center rounded-[var(--radius-sm)] border-none bg-transparent text-fg-subtle hover:bg-hover hover:text-fg"
              onClick={onClear}
              title="Close goal"
              aria-label="Close goal"
            >
              <XIcon size={13} />
            </button>
          )}
        </div>
      </div>
      <ComposerStripBody open={open}>
        <div
          className="min-w-0 truncate border-t border-border px-2.5 py-1.5 text-fg"
          title={goal.objective}
        >
          {goal.objective}
        </div>
      </ComposerStripBody>
    </div>
  );
}
