import { SquareIcon } from "@phosphor-icons/react";
import Spinner from "~/components/Spinner";
import type { BackgroundTask } from "~/api";

/**
 * Shell commands the agent started in the background and that are still running. They keep the
 * turn going after the reply ends, so the user can stop one from here.
 */
export default function BackgroundTasks({
  tasks,
  onStop,
}: {
  tasks: BackgroundTask[];
  onStop: (task: BackgroundTask) => void;
}) {
  if (tasks.length === 0) return null;
  return (
    <div className="mb-1.5 flex flex-col gap-0.5 px-2 text-[length:var(--fs-sm)]">
      {tasks.map((task) => (
        <div key={task.toolId} className="flex min-w-0 items-center gap-2">
          <Spinner className="text-accent" />
          <span className="flex-none font-semibold text-fg">Background</span>
          <span className="min-w-0 flex-1 truncate text-fg-muted">{task.description}</span>
          <button
            className="grid h-6 w-6 flex-none cursor-pointer place-items-center rounded-[var(--radius)] border-none bg-transparent text-danger hover:bg-hover"
            onClick={() => onStop(task)}
            title="Stop background command"
            aria-label={`Stop background command ${task.description}`}
          >
            <SquareIcon size={10} weight="fill" />
          </button>
        </div>
      ))}
    </div>
  );
}
