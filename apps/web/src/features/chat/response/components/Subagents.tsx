import { useState } from "react";
import { CaretDownIcon, CaretRightIcon } from "@phosphor-icons/react";
import { type AgentKind, type ToolCall } from "~/api";
import ProviderLogos from "~/components/ProviderLogos";
import { isActive, STATUS, titleOf } from "~/features/chat/lib/toolCalls";
import { fullElapsed } from "~/lib/util";
import Expander from "~/features/chat/response/components/Expander";
import { spanOf } from "~/features/chat/response/lib/model";
import { toolLabel } from "~/features/chat/response/lib/tools";
import { useNow } from "~/features/chat/hooks/useNow";

/**
 * The subagents a turn started: one as a card, several under a summary row that opens to list
 * them. They are their own step rather than rows in a work group, since each has a thread of its
 * own behind it.
 */
export default function Subagents({
  subagents,
  calls,
  agent,
  onOpenThread,
}: {
  subagents: readonly ToolCall[];
  /** Every call of the reply, including the ones the subagents made. */
  calls: readonly ToolCall[];
  agent: AgentKind;
  onOpenThread: (id: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const working = subagents.filter(isActive);
  const now = useNow(working.length > 0);
  const [only] = subagents;
  if (subagents.length === 1 && only !== undefined) {
    return (
      <div className="pb-2">
        <SubagentCard tool={only} calls={calls} agent={agent} onOpenThread={onOpenThread} />
      </div>
    );
  }
  const done = subagents.filter((t) => t.status === "done").length;
  const failed = subagents.filter((t) => t.status === "error").length;
  const summary = [
    working.length > 0 && `${working.length} working`,
    done > 0 && `${done} done`,
    failed > 0 && `${failed} failed`,
  ]
    .filter(Boolean)
    .join(" · ");
  const first = Math.min(...subagents.map((t) => t.startedAt ?? now));
  const elapsed = working.length > 0 ? now - first : spanOf(subagents);
  const active = working.length > 0;
  return (
    <div className="pb-2">
      <button
        type="button"
        aria-expanded={open}
        aria-label={`${subagents.length} subagents`}
        onClick={() => setOpen((o) => !o)}
        className={`flex w-full min-w-0 cursor-pointer items-center gap-2.5 rounded-[var(--radius-sm)] border-none bg-transparent px-2 py-1.5 text-left hover:bg-hover ${open || active ? "text-fg" : "text-fg-muted opacity-55"}`}
      >
        <span className="flex flex-none items-center -space-x-1.5">
          {subagents.slice(0, 3).map((tool) => (
            <Avatar key={tool.id} agent={agent} />
          ))}
          {subagents.length > 3 && (
            <span className="grid h-6 min-w-6 place-items-center rounded-full bg-panel px-1 text-[10px] text-fg-muted ring-2 ring-[var(--color-editor)]">
              +{subagents.length - 3}
            </span>
          )}
        </span>
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="truncate text-[length:var(--fs-xs)] font-semibold">
            {subagents.length} subagents
          </span>
          <span
            className={`truncate text-[length:var(--fs-xs)] ${failed > 0 ? "text-danger" : active ? "text-accent-text" : "text-fg-subtle"}`}
          >
            {summary}
          </span>
        </span>
        {elapsed !== null && (
          <span className="flex-none text-[length:var(--fs-xs)] text-fg-subtle tabular-nums">
            {fullElapsed(Math.max(0, elapsed))}
          </span>
        )}
        <CaretDownIcon
          size={14}
          className={`flex-none text-fg-muted transition-transform duration-200 ${open ? "rotate-180" : ""}`}
        />
      </button>
      {open && (
        <Expander>
          <div className="mt-1 flex flex-col gap-px rounded-[var(--radius)] border border-border bg-panel p-1">
            {subagents.map((tool) => (
              <SubagentCard
                key={tool.id}
                tool={tool}
                calls={calls}
                agent={agent}
                onOpenThread={onOpenThread}
              />
            ))}
          </div>
        </Expander>
      )}
    </div>
  );
}

function Avatar({ agent, dot }: { agent: AgentKind; dot?: string }) {
  return (
    <span className="relative grid h-6 w-6 flex-none place-items-center rounded-full border border-border bg-panel ring-2 ring-[var(--color-editor)]">
      <ProviderLogos kind={agent} size={13} />
      {dot && (
        <span
          className={`absolute -right-0.5 -bottom-0.5 h-2 w-2 rounded-full ring-2 ring-[var(--color-editor)] ${dot}`}
        />
      )}
    </span>
  );
}

/**
 * One subagent: what it was asked to do, what it is doing or found, and for how long. It opens its
 * own thread when it has one.
 */
function SubagentCard({
  tool,
  calls,
  agent,
  onOpenThread,
}: {
  tool: ToolCall;
  calls: readonly ToolCall[];
  agent: AgentKind;
  onOpenThread: (id: string) => void;
}) {
  const active = isActive(tool);
  const now = useNow(active);
  const status = STATUS[tool.status];
  const title = titleOf(tool);
  // While it works, what it is doing; once done, the first line of what it reported.
  const busy = [...calls].reverse().find((call) => call.parent === tool.id && isActive(call));
  const reported = tool.output
    ?.replace(/[`*_#>]/g, "")
    .split("\n")
    .map((line) => line.trim())
    .find(Boolean);
  const detail = active ? (busy ? toolLabel(busy) : undefined) : reported;
  const elapsed =
    tool.startedAt === null ? null : Math.max(0, (tool.endedAt ?? now) - tool.startedAt);
  const body = (
    <>
      <Avatar agent={agent} dot={status.dot} />
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="flex min-w-0 items-baseline gap-2">
          <span className="truncate text-[length:var(--fs-xs)] font-medium text-fg">{title}</span>
          {detail && (active || tool.status !== "done") && (
            <span
              className={`flex-none text-[10px] ${tool.status === "error" ? "text-danger" : "text-fg-subtle"}`}
            >
              {status.label}
            </span>
          )}
        </span>
        <span className="truncate text-[11px] text-fg-muted">{detail ?? status.label}</span>
      </span>
      {elapsed !== null && (
        <span className="flex-none text-[length:var(--fs-xs)] text-fg-subtle tabular-nums">
          {fullElapsed(elapsed)}
        </span>
      )}
      {tool.childThreadId && <CaretRightIcon size={14} className="flex-none text-fg-subtle" />}
    </>
  );
  const className =
    "flex w-full min-w-0 items-center gap-2.5 rounded-[var(--radius-sm)] border-none bg-transparent px-2 py-1.5 text-left";
  const childThreadId = tool.childThreadId;
  if (!childThreadId) return <div className={className}>{body}</div>;
  return (
    <button
      type="button"
      aria-label={`Open ${title}`}
      onClick={() => onOpenThread(childThreadId)}
      className={`${className} cursor-pointer hover:bg-hover`}
    >
      {body}
    </button>
  );
}
