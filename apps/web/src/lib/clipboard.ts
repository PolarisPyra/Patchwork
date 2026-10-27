import { api } from "~/api";

/** Puts `text` on the clipboard. The webview refuses a copy unless the page has focus and the
 * click was a fresh one, which a click that only brings the window forward is not, so the
 * system clipboard takes it then. */
export async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    await api.copyText(text);
  }
}
