import { invoke } from "~/api/invoke";
import type { Attachment } from "~/api/types";

/** What goes into a draft from a drop or a paste, and the clipboard behind it. */
export const attachmentsApi = {
  /** Classifies a dropped path as a file, folder, or image. */
  attachmentForPath: (path: string) => invoke<Attachment>("attachment_for_path", { path }),
  /** Saves a pasted image; `data` is the file's bytes, base64-encoded. */
  savePastedImage: (mimeType: string, data: string, name: string) =>
    invoke<Attachment>("save_pasted_image", { mimeType, data, name }),
  /** Saves the system clipboard's image to attach, or null when it holds none. */
  pasteClipboardImage: () => invoke<Attachment | null>("paste_clipboard_image"),
  /** The paths of files and folders copied in a file manager, or none. */
  clipboardFiles: () => invoke<string[]>("clipboard_files"),
  /** The clipboard copy's VS Code editor metadata (JSON), or null when it has none. */
  clipboardEditorData: () => invoke<string | null>("clipboard_editor_data"),
  /** Copies text through the system clipboard, for when the webview refuses to. */
  copyText: (text: string) => invoke<void>("copy_text", { text }),
  /** Saves a long pasted text as a file to attach. */
  savePastedText: (text: string) => invoke<Attachment>("save_pasted_text", { text }),
  /** Lets the webview show an attached image as a thumbnail. */
  allowPreview: (path: string) => invoke<void>("allow_preview", { path }),
};
