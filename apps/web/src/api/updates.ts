import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "~/api/invoke";

/** Where updating Patchwork stands. Mirrors `UpdateState` in apps/desktop/src-tauri/src/updates/state.rs. */
export type UpdateState =
  | { status: "idle" }
  | { status: "checking" }
  | { status: "upToDate" }
  /** A newer release this copy cannot install itself. */
  | { status: "available"; version: string }
  | { status: "downloading"; version: string; downloaded: number; total: number | null }
  /** Installed; it runs once Patchwork restarts. */
  | { status: "ready"; version: string }
  | { status: "failed"; message: string };

export interface UpdateStatus {
  currentVersion: string;
  /** Whether this copy can install updates itself; a dev or source build cannot. */
  canInstall: boolean;
  state: UpdateState;
  /** The notes of the newer release the last check found, as Markdown. */
  notes: string | null;
}

export type RestartResult =
  { result: "busy"; running: number } | { result: "nothingReady" } | { result: "restarting" };

export const updatesApi = {
  updateStatus: () => invoke<UpdateStatus>("update_status"),
  /** Starts a check; its progress and result arrive through `onUpdateStatus`. */
  checkForUpdates: () => invoke<void>("check_for_updates"),
  /** Restarts into the installed update; without `force`, refuses while agents are replying. */
  restartToUpdate: (force: boolean) => invoke<RestartResult>("restart_to_update", { force }),
};

export function onUpdateStatus(cb: (status: UpdateStatus) => void): Promise<UnlistenFn> {
  return listen<UpdateStatus>("update-status", (e) => cb(e.payload));
}
