import { useState } from "react";
import { CaretRightIcon } from "@phosphor-icons/react";
import { type Thread, type ToolCall } from "~/api";
import LineageMenu from "~/features/chat/lineage/components/LineageMenu";
import SubagentRow from "~/features/chat/lineage/components/SubagentRow";
import { currentTurnTools, isActive } from "~/features/chat/lib/toolCalls";
import { useNow } from "~/features/chat/hooks/useNow";

/** Subagents of the current turn, split into running ones and previous ones, with a disconnect menu. */
export default function LineageCard({
  thread,
  working,
  onDisconnect,
  onError,
}: {
  thread: Thread;
  working: boolean;
  onDisconnect: () => void;
  onError: (message: string) => void;
}) {
  // The newest subagent per thread when the card was closed; it reopens once another starts.
  const [closedAt, setClosedAt] = useState<Record<string, string>>({});
  const [previousOpen, setPreviousOpen] = useState(false);

  const tools = currentTurnTools(thread);
  const subagents = tools.filter((t) => t.subagent);
  // Ticks the elapsed times once a second, only while a subagent is still going.
  const now = useNow(subagents.some(isActive));

  const newest = subagents.at(-1)?.id;
  if (newest === undefined || closedAt[thread.id] === newest) return null;
  const shownUpTo = newest;

  const active = subagents.filter(isActive);
  const previous = subagents.filter((t) => !isActive(t));
  const running = subagents.filter((t) => t.status === "running").length;

  function rows(list: ToolCall[]) {
    return (
      <div className="flex flex-col gap-px px-1.5 pb-1.5">
        {list.map((tool) => (
          <SubagentRow
            key={tool.id}
            tool={tool}
            thread={thread}
            tools={tools}
            now={now}
            onError={onError}
          />
        ))}
      </div>
    );
  }

  return (
    <div className="absolute top-3 right-4 z-10 w-[280px] rounded-[var(--radius-lg)] border border-border bg-panel text-[length:var(--fs-sm)] shadow-[var(--shadow-popup)]">
      <div className="flex items-center gap-2 px-3 py-2">
        <span className="flex-1 truncate font-medium text-[length:var(--fs-xs)] text-fg-muted">
          {running > 0 ? `Lineage · ${running} running` : "Lineage"}
        </span>
        <LineageMenu
          working={working}
          onDisconnect={onDisconnect}
          onClose={() => setClosedAt((c) => ({ ...c, [thread.id]: shownUpTo }))}
        />
      </div>
      {active.length > 0 && rows(active)}
      {previous.length > 0 && (
        <div className="border-t border-border">
          <button
            type="button"
            className="flex w-full cursor-pointer items-center gap-1.5 border-none bg-transparent px-3 py-1.5 text-left text-[length:var(--fs-xs)] text-fg-muted hover:text-fg"
            aria-expanded={previousOpen}
            onClick={() => setPreviousOpen((o) => !o)}
          >
            <CaretRightIcon size={10} className={previousOpen ? "rotate-90" : undefined} />
            Previous agents
            {!previousOpen && ` (${previous.length})`}
          </button>
          {previousOpen && rows(previous)}
        </div>
      )}
    </div>
  );
}
