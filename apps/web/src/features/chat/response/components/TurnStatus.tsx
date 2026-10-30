import { ArrowClockwiseIcon, TargetIcon } from "@phosphor-icons/react";
import Spinner from "~/components/Spinner";
import { LineGutter } from "~/features/chat/response/components/AgentStepDisplay";
import { type Phase } from "~/features/chat/response/lib/model";
import { useNow } from "~/features/chat/hooks/useNow";
import { fullElapsed } from "~/lib/util";

/**
 * The one status line a turn has: what is happening now, above what has already happened.
 *
 * It is the same shape as the line that replaces it once the turn settles (see FoldHead), so a
 * finished turn reads as one line changing what it says rather than two lines swapping places:
 * "Thinking for 9s" becomes "Thought for 9s". While the agent thinks, a grey band sweeps across
 * the words instead of a spinner beside them, so they sit where the settled line's words will.
 * `tabular-nums` keeps the digits from shifting as the count grows.
 */
export default function TurnStatus({ phase, since }: { phase: Phase; since: number }) {
  const now = useNow(phase.kind === "working");
  const label =
    phase.kind === "retry"
      ? `Retrying: ${phase.reason} (${phase.attempt}/${phase.maxAttempts})`
      : phase.kind === "checking-goal"
        ? "Checking goal"
        : phase.kind === "compacting"
          ? "Compacting context"
          : `Thinking for ${fullElapsed(Math.max(0, now - since))}`;
  return (
    <div className="mb-1.5 border-b border-border pt-1 pb-2">
      <div className="flex h-6 min-w-0 items-center gap-1.5 text-[length:var(--fs-md)] text-fg-muted tabular-nums">
        {phase.kind === "compacting" ? (
          // In the line gutter, like a running row's spinner, so its words line up with the rows'.
          <LineGutter>
            <Spinner size={11} className="translate-y-px text-fg-subtle" />
          </LineGutter>
        ) : phase.kind !== "working" ? (
          <LineGutter>
            {phase.kind === "retry" ? <ArrowClockwiseIcon size={15} /> : <TargetIcon size={15} />}
          </LineGutter>
        ) : null}
        <span className={`min-w-0 truncate ${phase.kind === "working" ? "text-shimmer" : ""}`}>
          {label}
        </span>
      </div>
    </div>
  );
}
