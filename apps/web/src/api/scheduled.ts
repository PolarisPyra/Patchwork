import { invoke } from "~/api/invoke";
import type { ScheduledTask, Thread } from "~/api/types";

export const scheduledApi = {
  /** The scheduled tasks, as saved. */
  listScheduledTasks: () => invoke<ScheduledTask[]>("list_scheduled_tasks"),
  /** Adds or replaces a task, and returns it with its next run worked out. */
  saveScheduledTask: (task: ScheduledTask) =>
    invoke<ScheduledTask>("save_scheduled_task", { task }),
  removeScheduledTask: (id: string) => invoke<void>("remove_scheduled_task", { id }),
  /** Starts a task now, whatever its schedule says. */
  runScheduledTask: (id: string) => invoke<Thread>("run_scheduled_task", { id }),
};
