import { useRef } from "react";
import type { Editor } from "@tiptap/react";
import type { HistoryEntry } from "~/features/chat/composer/types";

/**
 * Recalls the chat's earlier messages into the draft with Up and Down. `step` takes the arrow key
 * and returns true when it recalled a message.
 */
export function useHistoryRecall({
  history,
  value,
  hasAttachments,
  onChange,
}: {
  /** The chat's earlier messages, oldest first. */
  history: HistoryEntry[];
  /** The draft as Markdown. */
  value: string;
  hasAttachments: boolean;
  onChange: (text: string) => void;
}) {
  // The recalled history entry, while the draft still holds it unchanged.
  const recalled = useRef<HistoryEntry | null>(null);

  /** Up and Down recall earlier messages only from the first or last line of the draft, counting
   * wrapped lines. Anywhere else they move the caret a line at a time, so a multi-line draft or a
   * recalled message can be walked through before the next one is reached. */
  function step(event: KeyboardEvent, editor: Editor): boolean {
    const back = event.key === "ArrowUp";
    if (
      (!back && event.key !== "ArrowDown") ||
      event.shiftKey ||
      event.altKey ||
      event.metaKey ||
      event.ctrlKey
    ) {
      return false;
    }
    if (hasAttachments) return false;
    const { selection, doc } = editor.state;
    if (!selection.empty) return false;
    const at = recalled.current?.text === value ? recalled.current : null;
    // The caret must be in the first (Up) or last (Down) block, on that block's top or bottom line.
    let edgeBlock = -1;
    doc.descendants((node, pos) => {
      if (node.isTextblock && (!back || edgeBlock < 0)) edgeBlock = pos;
      return !node.isTextblock;
    });
    if (selection.$from.before() !== edgeBlock) return false;
    if (!editor.view.endOfTextblock(back ? "up" : "down")) return false;
    const index = at ? history.findIndex((h) => h.id === at.id) : -1;
    if (index < 0 && (value.length > 0 || !back)) return false;
    const next = back ? history[index < 0 ? history.length - 1 : index - 1] : history[index + 1];
    if (back && !next) return false;
    recalled.current = next ?? null;
    onChange(next?.text ?? "");
    return true;
  }

  return step;
}
