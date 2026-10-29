import { invoke } from "~/api/invoke";
import type { FileMedia, TurnFile, TurnTrees } from "~/api/types";

export const filesApi = {
  readFile: (projectId: string, path: string) => invoke<string>("read_file", { projectId, path }),
  /** Which of `paths` no longer exist in the project. */
  missingFiles: (projectId: string, paths: string[]) =>
    invoke<string[]>("missing_files", { projectId, paths }),
  /** What each of a reply's files is shown as: a thumbnail, a card, or a line in the diff list. */
  fileMedia: (projectId: string, paths: string[]) =>
    invoke<FileMedia[]>("file_media", { projectId, paths }),
  /** Opens a file inside the project in the default app. */
  openFile: (projectId: string, path: string) => invoke<void>("open_file", { projectId, path }),
  turnFiles: (projectId: string, turn: TurnTrees, paths: string[]) =>
    invoke<TurnFile[]>("turn_files", { projectId, turn, paths }),
  turnFileDiff: (projectId: string, turn: TurnTrees, path: string) =>
    invoke<string>("turn_file_diff", { projectId, turn, path }),
  /** Writes `text` to `path`, a file the user picked in a save dialog. */
  saveTextFile: (path: string, text: string) => invoke<void>("save_text_file", { path, text }),
  openInEditor: (projectId: string, path: string) =>
    invoke<void>("open_in_editor", { projectId, path }),
  /** Shows a file selected in the file manager; a relative path is taken from the project's root. */
  revealFile: (projectId: string | null, path: string) =>
    invoke<void>("reveal_file", { projectId, path }),
  /** Opens a web or mail link in the default browser or mail app. */
  openUrl: (url: string) => invoke<void>("open_url", { url }),
};
