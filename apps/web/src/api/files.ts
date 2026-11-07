import { getCurrentWebview } from "@tauri-apps/api/webview";
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

type Point = { x: number; y: number };

/**
 * Follows files dragged from the file manager over the window. Tauri hands dropped files over as
 * paths (HTML drop events carry none), so drops are listened for on the webview. `over` says
 * whether a point, in CSS pixels, is on the target; `onHover` hears whether a drag is over it, and
 * `onDrop` gets the files dropped on it. Returns the stop.
 */
export function onFileDrop(
  over: (point: Point) => boolean,
  onHover: (hovering: boolean) => void,
  onDrop: (paths: string[], point: Point) => void,
): () => void {
  let cancelled = false;
  let unlisten: (() => void) | undefined;
  void getCurrentWebview()
    .onDragDropEvent(({ payload }) => {
      if (payload.type === "leave") {
        onHover(false);
        return;
      }
      // Positions are physical pixels; the page's are CSS pixels.
      const scale = window.devicePixelRatio;
      const point = { x: payload.position.x / scale, y: payload.position.y / scale };
      if (payload.type === "drop") {
        onHover(false);
        if (over(point)) onDrop(payload.paths, point);
      } else {
        onHover(over(point));
      }
    })
    .then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
  return () => {
    cancelled = true;
    unlisten?.();
  };
}
