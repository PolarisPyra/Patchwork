import { useEffect, useState } from "react";
import { SquareIcon } from "@phosphor-icons/react";
import Spinner from "~/components/Spinner";
import type { BackgroundTask } from "~/api";

/** What each kind of task the agents report is called here. Any other kind is shown as it comes. */
const KIND_LABELS: Record<string, string> = {
  command: "Command",
  local_bash: "Command",
  agent: "Agent",
  local_agent: "Agent",
  local_workflow: "Workflow",
  mcp_task: "MCP",
  local_memory_import: "Memory",
};

/** Elapsed time as m:ss, or h:mm:ss from an hour on. */
function elapsed(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = String(total % 60).padStart(2, "0");
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${seconds}`
    : `${minutes}:${seconds}`;
}

/**
 * Tasks the agent started that are still running after its reply ended. They keep the turn going,
 * so the user can follow them and stop one from here. A task the agent has no way to stop has no
 * stop button.
 */
export default function BackgroundTasks({
  tasks,
  onStop,
}: {
  tasks: BackgroundTask[];
  onStop: (task: BackgroundTask) => void;
}) {
  const live = tasks.filter((task) => task.status === "running" || task.status === "waiting");
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (live.length === 0) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [live.length]);
  if (live.length === 0) return null;
  return (
    <div
      role="region"
      aria-label="Background tasks"
      className="mb-1.5 flex flex-col gap-0.5 px-2 text-[length:var(--fs-sm)]"
    >
      {live.map((task) => (
        <div key={task.toolId} className="flex min-w-0 items-center gap-2">
          {task.status === "running" ? (
            <Spinner className="text-accent" />
          ) : (
            <span className="flex-none text-fg-muted">Waiting</span>
          )}
          <span className="flex-none font-semibold text-fg">
            {KIND_LABELS[task.kind] ?? task.kind}
          </span>
          <span className="min-w-0 flex-1 truncate text-fg-muted">{task.description}</span>
          {task.summary && (
            <span className="min-w-0 max-w-[40%] flex-none truncate text-fg-muted">
              {task.summary}
            </span>
          )}
          <span className="flex-none tabular-nums text-fg-muted">
            {elapsed(now - task.startedAt)}
          </span>
          {task.taskId !== null && (
            <button
              className="grid h-6 w-6 flex-none cursor-pointer place-items-center rounded-[var(--radius)] border-none bg-transparent text-danger hover:bg-hover"
              onClick={() => onStop(task)}
              title="Stop background command"
              aria-label={`Stop background command ${task.description}`}
            >
              <SquareIcon size={10} weight="fill" />
            </button>
          )}
        </div>
      ))}
    </div>
  );
}
