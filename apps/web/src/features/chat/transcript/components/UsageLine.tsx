import { WarningIcon } from "@phosphor-icons/react";
import { type TokenUsage } from "~/api";
import { formatTokens } from "~/lib/tokens";

// Markdown loads with the chat, not on first use: its plain-text placeholder had another height,
// so swapping it in moved the messages.

const LINE_CLASS =
  "m-0 mb-1.5 flex flex-wrap items-center justify-end gap-x-3 pr-3 text-[length:var(--fs-xs)] tabular-nums";

/**
 * The thread's tokens, shown above the composer. A context over `warnAbove` is flagged, since every
 * turn re-reads it. Before the first reply reports any, the line holds its place, empty, so its
 * arrival mid-turn does not push the chat up.
 */
export function UsageLine({ usage, warnAbove }: { usage: TokenUsage | null; warnAbove: number }) {
  if (usage === null) {
    return (
      <p className={`${LINE_CLASS} invisible`} aria-hidden>
        <span>Context</span>
      </p>
    );
  }
  const large = usage.context > warnAbove;
  return (
    <p className={`${LINE_CLASS} ${large ? "text-warning" : "text-fg-muted"}`}>
      <span className="inline-flex items-center gap-1">
        {large && <WarningIcon size={13} />}
        {large
          ? `Context over ${formatTokens(warnAbove)}`
          : `Context ${formatTokens(usage.context)}`}
      </span>
      <span>Cache read {formatTokens(usage.cacheRead)} tokens</span>
      <span>Cache write {formatTokens(usage.cacheWrite)} tokens</span>
      <span>Input {formatTokens(usage.input)} tokens</span>
      <span>Output {formatTokens(usage.output)} tokens</span>
      {usage.firstTokenMs !== undefined && (
        <span>
          First token{" "}
          {usage.firstTokenMs < 1000
            ? `${usage.firstTokenMs} ms`
            : `${(usage.firstTokenMs / 1000).toFixed(1)} s`}
        </span>
      )}
      {usage.outputPerSecond !== undefined && <span>{usage.outputPerSecond} tokens/s</span>}
    </p>
  );
}
