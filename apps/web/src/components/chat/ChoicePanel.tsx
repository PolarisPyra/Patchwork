import { useEffect, useRef, useState } from "react";

interface ChoicePanelProps {
  /** What the agent asks, shown above the options. */
  question?: string;
  /** The options the agent offered. */
  options: string[];
  /** A line explaining each option, by index. */
  descriptions?: string[];
  /** Sends the chosen option as the next message. */
  onPick: (option: string) => void;
  /** Hides the panel; the composer then unlocks and takes the focus to write an answer. */
  onOwn: () => void;
  /** Hides the panel without an answer, on Escape. */
  onDismiss: () => void;
}

/**
 * Numbered options above the composer. A number key or the arrows highlight an option; Enter
 * sends the highlighted one. The last row writes an answer instead of sending.
 */
export default function ChoicePanel({
  question,
  options,
  descriptions,
  onPick,
  onOwn,
  onDismiss,
}: ChoicePanelProps) {
  // Rows are the options, then "Submit my own response" at index options.length.
  const own = options.length;
  const [active, setActive] = useState<number | null>(null);
  const activeRef = useRef(active);
  activeRef.current = active;

  // The composer is read-only while the panel is open, so the panel always takes these keys.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return;
      // A field the user is typing in keeps its keys.
      const target = e.target as HTMLElement | null;
      if (target?.matches?.("input, textarea, select")) return;

      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onDismiss();
        return;
      }

      const current = activeRef.current;
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        e.stopPropagation();
        const rows = own + 1;
        const next =
          current === null
            ? e.key === "ArrowDown"
              ? 0
              : rows - 1
            : (current + (e.key === "ArrowDown" ? 1 : rows - 1)) % rows;
        setActive(next);
        return;
      }
      if (e.key === "Enter" && current !== null) {
        e.preventDefault();
        e.stopPropagation();
        if (current === own) onOwn();
        else onPick(options[current]);
        return;
      }
      const n = Number(e.key);
      if (Number.isInteger(n) && n >= 1 && n <= own + 1) {
        e.preventDefault();
        e.stopPropagation();
        setActive(n - 1);
      }
    };
    // Capture phase, so the editor does not also act on these keys (history, send).
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [options, onPick, onOwn, onDismiss, own]);

  const rowClass = (highlighted: boolean) =>
    `flex w-full cursor-pointer items-center gap-3 rounded-[var(--radius-sm)] border-none px-2.5 py-1.5 text-left text-[length:var(--fs-md)] text-fg ${highlighted ? "bg-selected" : "bg-transparent hover:bg-hover"}`;

  return (
    <div
      role="listbox"
      aria-label="Choose a reply"
      className="mb-1.5 flex flex-col gap-px rounded-[var(--radius)] border border-border bg-panel p-1"
    >
      {question && (
        <p className="m-0 px-2.5 pt-1 pb-1.5 text-[length:var(--fs-md)] font-semibold text-fg selectable">
          {question}
        </p>
      )}
      {options.map((option, i) => (
        <button
          key={option}
          role="option"
          aria-selected={active === i}
          className={rowClass(active === i)}
          onClick={() => setActive(i)}
        >
          <kbd className="kbd flex-none">{i + 1}</kbd>
          <span className="min-w-0 flex-1 truncate">{option}</span>
          {descriptions?.[i] && (
            <span className="min-w-0 flex-[2] truncate text-[length:var(--fs-sm)] text-fg-muted">
              {descriptions[i]}
            </span>
          )}
        </button>
      ))}
      <button
        role="option"
        aria-selected={active === own}
        className={rowClass(active === own)}
        onClick={onOwn}
      >
        <kbd className="kbd flex-none">{own + 1}</kbd>
        <span className="min-w-0 flex-1 truncate text-fg-muted">Submit my own response</span>
      </button>
    </div>
  );
}
