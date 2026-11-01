import { useEffect, useRef, useState } from "react";
import { SquareIcon } from "@phosphor-icons/react";
import { api, type Thread, type ToolCall } from "~/api";
import ProviderLogos from "~/components/ProviderLogos";
import DetailsCard from "~/features/chat/lineage/DetailsCard";
import { elapsedMs, STATUS, titleOf } from "~/features/chat/lib/toolCalls";
import { compactElapsed, errorText } from "~/lib/util";

/** How long a row is hovered before its details card opens. */
const HOVER_DELAY_MS = 200;

/** One subagent: provider avatar with a status dot, title, elapsed time, and a stop button while running. */
export default function SubagentRow({
  tool,
  thread,
  tools,
  now,
  onError,
}: {
  tool: ToolCall;
  thread: Thread;
  tools: ToolCall[];
  now: number;
  onError: (message: string) => void;
}) {
  const [hovered, setHovered] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(timer.current), []);

  const elapsed = elapsedMs(tool, now);
  const stoppable = tool.status === "running" && thread.agent === "claude";
  const title = titleOf(tool);

  function enter() {
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setHovered(true), HOVER_DELAY_MS);
  }

  function leave() {
    window.clearTimeout(timer.current);
    setHovered(false);
  }

  function stop() {
    api.stopSubagent(thread.id, tool.id).catch((e: unknown) => onError(errorText(e)));
  }

  return (
    <div
      className="group relative flex min-w-0 items-center gap-2 rounded-[var(--radius)] px-1.5 py-1 hover:bg-hover"
      onMouseEnter={enter}
      onMouseLeave={leave}
      // Focus shows the details too, so the keyboard reaches what hovering shows.
      tabIndex={0}
      onFocus={() => setHovered(true)}
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget)) leave();
      }}
    >
      <span className="relative grid h-5 w-5 flex-none place-items-center">
        <ProviderLogos kind={thread.agent} model={thread.model} size={16} />
        <span
          className={`absolute right-0 bottom-0 h-[7px] w-[7px] rounded-full ring-2 ring-[var(--color-panel)] ${STATUS[tool.status].dot}`}
        />
      </span>
      <span className="min-w-0 flex-1 truncate text-[length:var(--fs-sm)] font-medium text-fg">
        {title}
      </span>
      <span className="relative flex flex-none items-center justify-end">
        {tool.status === "error" ? (
          <span className="text-[length:var(--fs-xs)] text-danger">{STATUS.error.label}</span>
        ) : (
          elapsed !== null && (
            <span
              className={`tabular-nums text-[length:var(--fs-xs)] text-fg-subtle${stoppable ? " transition-opacity group-hover:opacity-0" : ""}`}
            >
              {compactElapsed(elapsed)}
            </span>
          )
        )}
        {stoppable && (
          <button
            type="button"
            className="absolute top-1/2 right-0 grid h-5 w-5 -translate-y-1/2 cursor-pointer place-items-center rounded-[var(--radius)] border-none bg-transparent p-0 text-danger opacity-0 hover:bg-hover focus-visible:opacity-100 group-hover:opacity-100"
            aria-label={`Stop subagent ${title}`}
            title="Stop subagent"
            onClick={stop}
          >
            <SquareIcon size={10} weight="fill" />
          </button>
        )}
      </span>
      {hovered && <DetailsCard tool={tool} tools={tools} elapsed={elapsed} />}
    </div>
  );
}
