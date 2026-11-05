import { invoke } from "~/api/invoke";
import type { BackgroundImage, ChatBackground, RemovedBackground } from "~/api/types";

export const backgroundsApi = {
  /** Gives a chat its own background, or null to show the one in Settings. Needs a plan with
   * per-chat backgrounds, unless `background` is null. */
  setThreadBackground: (id: string, background: ChatBackground | null) =>
    invoke<void>("set_thread_background", { id, background }),
  /** Copies a picture into Patchwork's folder, shrunk to fit, so it stays when the original moves.
   * Resolves to the copy's file name, the same one when the picture was picked before. */
  importBackgroundImage: (path: string) => invoke<string>("import_background_image", { path }),
  /** Saves the picture on the system clipboard like `importBackgroundImage`; null when there is
   * none. */
  pasteBackgroundImage: () => invoke<string | null>("paste_background_image"),
  /** The pictures in Patchwork's folder, oldest first. */
  listBackgroundImages: () => invoke<BackgroundImage[]>("list_background_images"),
  /** Deletes a picture. The chats showing it go back to the shared background, and the shared
   * background, if it was this picture, to none. Resolves to what `restoreBackgroundImage` needs
   * to take it back. */
  removeBackgroundImage: (file: string) =>
    invoke<RemovedBackground>("remove_background_image", { file }),
  /** Brings a deleted picture back, with the backgrounds it was. */
  restoreBackgroundImage: (removed: RemovedBackground) =>
    invoke<void>("restore_background_image", { removed }),
};
