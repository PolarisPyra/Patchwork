import { CheckIcon, TerminalIcon, XCircleIcon } from "@phosphor-icons/react";
import Spinner from "~/components/Spinner";
import { type ToolCall } from "~/api";
import { STATUS, titleOf } from "~/features/chat/lib/toolCalls";
import { fullElapsed } from "~/lib/util";

/** Floating card to the left of a row: what the subagent is and what it did last. */
export default function DetailsCard({
  tool,
  tools,
  elapsed,
}: {
  tool: ToolCall;
  tools: ToolCall[];
  elapsed: number | null;
}) {
  const actions = tools.filter((t) => t.parent === tool.id && !t.subagent);
  const last = actions[actions.length - 1];
  return (
    <div className="pointer-events-none absolute top-0 right-full z-20 mr-2 flex w-64 flex-col gap-1.5 rounded-[var(--radius)] border border-border bg-panel p-2.5 text-[length:var(--fs-sm)] shadow-[var(--shadow-popup)]">
      <div className="font-semibold break-words">{titleOf(tool)}</div>
      <div className="text-fg-muted">
        {`${tool.subagent?.model ?? "inherits model"} · ${tool.subagent?.kind ?? ""}`}
      </div>
      <div className="flex items-center gap-1.5">
        {tool.status === "running" && (
          <>
            <Spinner className="text-accent" />
            <span className="text-accent">{STATUS.running.label}</span>
          </>
        )}
        {tool.status === "done" && (
          <>
            <CheckIcon size={12} className="flex-none text-success" />
            <span className="text-success">{STATUS.done.label}</span>
          </>
        )}
        {tool.status === "error" && (
          <>
            <XCircleIcon size={12} className="flex-none text-danger" />
            <span className="text-danger">{STATUS.error.label}</span>
          </>
        )}
        {tool.status === "pending" && (
          <span className="text-fg-subtle">{STATUS.pending.label}</span>
        )}
        {elapsed !== null && (
          <span className="ml-auto tabular-nums text-fg-muted">{fullElapsed(elapsed)}</span>
        )}
      </div>
      {last && (
        <div className="flex min-w-0 items-center gap-1.5 font-[family-name:var(--font-mono)] text-[length:var(--fs-xs)] text-fg-muted">
          <TerminalIcon size={12} className="flex-none" />
          <span className="truncate">{`${last.name} ${last.summary}`}</span>
        </div>
      )}
    </div>
  );
}
