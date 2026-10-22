// Highlights a file's lines off the main thread, so opening a long file does not stall the UI.
import { highlightLines } from "~/components/canvas/highlight";

self.onmessage = (e: MessageEvent<{ id: number; lines: string[]; grammar: string }>) => {
  const { id, lines, grammar } = e.data;
  self.postMessage({ id, html: highlightLines(lines, grammar) });
};
