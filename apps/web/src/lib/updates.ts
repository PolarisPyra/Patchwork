import { confirm } from "@tauri-apps/plugin-dialog";
import { api } from "~/api";

export const RELEASES_URL = "https://github.com/PolarisPyra/Patchwork-ADE/releases/latest";

/** Restarts into the installed update. Running agents stop for the restart, and carry on after
 * it, so the user confirms that first. */
export async function restartToUpdate(): Promise<void> {
  const result = await api.restartToUpdate(false);
  if (result.result !== "busy") return;
  const tasks = result.running === 1 ? "1 task is" : `${result.running} tasks are`;
  const ok = await confirm(
    `${tasks} still running. They stop now and carry on after the restart.`,
    {
      title: "Restart to update",
      kind: "warning",
    },
  );
  if (ok) await api.restartToUpdate(true);
}
