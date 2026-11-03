import { useEffect, useState, type RefObject } from "react";
import { QuotesIcon } from "@phosphor-icons/react";
import { selectionMarkdown } from "~/features/chat/lib/selectionMarkdown";
import { readTextSelection, selectedText } from "~/features/chat/lib/textSelection";

/** The text selected in `root`, as the lines of a Markdown quote, or null when none is. */
export function quotedSelection(root: HTMLElement): string[] | null {
  const range = readTextSelection(root);
  if (!range) return null;
  const text = (selectionMarkdown(range)?.markdown ?? selectedText(root, range)).trim();
  return text ? text.split("\n").map((line) => (line ? `> ${line}` : ">")) : null;
}

/**
 * A Quote button over text selected in the chat. It puts the text in the composer as a quote,
 * the way `quoteSelection` does from the keyboard.
 */
export default function QuoteSelection({
  root,
  onQuote,
}: {
  root: RefObject<HTMLDivElement | null>;
  onQuote: (lines: string[]) => void;
}) {
  // Where the selection's top middle is, inside `root`.
  const [at, setAt] = useState<{ top: number; left: number } | null>(null);
  useEffect(() => {
    let frame = 0;
    const place = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        const el = root.current;
        const rect = el && readTextSelection(el)?.getBoundingClientRect();
        const box = el?.getBoundingClientRect();
        // Hidden once the selection's top scrolls out of the chat.
        const shown = rect && box && rect.width > 0 && rect.top >= box.top && rect.top < box.bottom;
        setAt(
          shown ? { top: rect.top - box.top, left: rect.left + rect.width / 2 - box.left } : null,
        );
      });
    };
    document.addEventListener("selectionchange", place);
    window.addEventListener("scroll", place, true);
    window.addEventListener("resize", place);
    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener("selectionchange", place);
      window.removeEventListener("scroll", place, true);
      window.removeEventListener("resize", place);
    };
  }, [root]);

  if (!at) return null;
  return (
    <button
      type="button"
      style={{ top: Math.max(at.top - 6, 0), left: at.left }}
      className="absolute z-10 flex -translate-x-1/2 -translate-y-full cursor-pointer items-center gap-1 rounded-[var(--radius)] border border-border bg-panel px-2 py-1 text-[length:var(--fs-sm)] text-fg shadow-[var(--shadow-popup)] hover:bg-hover"
      // A click would clear the selection before it is read.
      onMouseDown={(e) => e.preventDefault()}
      onClick={() => {
        const lines = root.current && quotedSelection(root.current);
        if (lines) onQuote(lines);
      }}
    >
      <QuotesIcon size={14} aria-hidden />
      Quote
    </button>
  );
}
