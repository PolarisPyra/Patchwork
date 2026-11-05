import { confirm } from "@tauri-apps/plugin-dialog";
import { api } from "~/api";

export const RELEASES_URL = "https://github.com/PolarisPyra/Patchwork/releases/latest";
/** The licenses the app ships under, which its version pane links to: the AGPL for the core, and
 * the commercial license for the paid features in `commercial/`. */
export const LICENSE_URL = "https://www.gnu.org/licenses/agpl-3.0.html";
export const COMMERCIAL_LICENSE_URL =
  "https://github.com/PolarisPyra/Patchwork/blob/main/LICENSE_COMMERCIAL.txt";

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
