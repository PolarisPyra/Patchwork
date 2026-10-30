import { ArrowClockwiseIcon, TargetIcon } from "@phosphor-icons/react";
import Spinner from "~/components/Spinner";
import { type Phase } from "~/features/chat/turn/lib/model";
import { useNow } from "~/features/chat/hooks/useNow";
import { fullElapsed } from "~/lib/util";

/**
 * The one status line a turn has: what is happening now, above what has already happened.
 *
 * It is the same shape as the line that replaces it once the turn settles (see FoldHead), so a
 * finished turn reads as one line changing what it says rather than two lines swapping places.
 * `tabular-nums` keeps the digits from shifting as the count grows.
 */
export default function StatusLine({ phase, since }: { phase: Phase; since: number }) {
  const now = useNow(phase.kind === "working");
  const label =
    phase.kind === "retry"
      ? `Retrying: ${phase.reason} (${phase.attempt}/${phase.maxAttempts})`
      : phase.kind === "checking-goal"
        ? "Checking goal"
        : phase.kind === "compacting"
          ? "Compacting context"
          : `Working for ${fullElapsed(Math.max(0, now - since))}`;
  return (
    <div className="mb-1.5 border-b border-border pt-1 pb-2">
      <div className="flex h-6 min-w-0 items-center gap-1.5 px-1 text-[length:var(--fs-md)] text-fg-muted tabular-nums">
        <span className="flex flex-none opacity-70">
          {phase.kind === "retry" ? (
            <ArrowClockwiseIcon size={15} />
          ) : phase.kind === "checking-goal" ? (
            <TargetIcon size={15} />
          ) : (
            <Spinner size={13} className="text-fg-subtle" />
          )}
        </span>
        <span className="min-w-0 truncate">{label}</span>
      </div>
    </div>
  );
}
