import { invoke } from "~/api/invoke";
import type { ChatBackground } from "~/api/types";

export const backgroundsApi = {
  /** Gives a chat its own background, or null to show the one in Settings. Needs a plan with
   * per-chat backgrounds, unless `background` is null. */
  setThreadBackground: (id: string, background: ChatBackground | null) =>
    invoke<void>("set_thread_background", { id, background }),
  /** Copies a picture into Patchwork's folder, so it stays when the original moves. Resolves to
   * the copy's path. */
  importBackgroundImage: (path: string) => invoke<string>("import_background_image", { path }),
};
