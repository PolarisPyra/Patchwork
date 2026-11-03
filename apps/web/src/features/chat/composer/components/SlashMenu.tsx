import type { RefObject } from "react";
import type { SlashMatch, SlashMenuKind } from "~/features/chat/composer/hooks/useSlashMenu";

const LABELS: Record<SlashMenuKind, { name: string; empty: string }> = {
  command: { name: "Slash commands", empty: "No matching command." },
  skill: { name: "Skills", empty: "No matching skill." },
  agent: { name: "Agents", empty: "No matching agent." },
};

/** The commands, skills or agents that match the word being typed, above the composer. */
export default function SlashMenu({
  kind,
  matches,
  active,
  activeRef,
  sigil,
  onSelect,
}: {
  kind: SlashMenuKind;
  matches: SlashMatch[];
  /** The highlighted entry's name. */
  active: string | undefined;
  /** Set on the highlighted entry, so it can be scrolled into view. */
  activeRef: RefObject<HTMLButtonElement | null>;
  /** The sigil that was typed, which may be the user's own, or null to show the default. */
  sigil: string | null;
  onSelect: (name: string) => void;
}) {
  return (
    <div
      role="listbox"
      aria-label={LABELS[kind].name}
      className="absolute right-0 bottom-full left-0 mb-1.5 flex max-h-[min(50vh,32rem)] flex-col gap-px overflow-y-auto overscroll-contain rounded-[var(--radius)] border border-border bg-panel p-1 shadow-[var(--shadow-popup)]"
    >
      {matches.length === 0 && (
        <div className="px-2 py-1.5 text-[length:var(--fs-sm)] text-fg-muted">
          {LABELS[kind].empty}
        </div>
      )}
      {matches.map((c) => (
        <button
          key={c.name}
          ref={c.name === active ? activeRef : undefined}
          role="option"
          aria-selected={c.name === active}
          className={`flex w-full flex-none cursor-pointer items-baseline gap-2.5 rounded-[var(--radius)] border-none px-2 py-1.5 text-left ${c.name === active ? "bg-selected" : "bg-transparent hover:bg-hover"}`}
          // Keeps the focus in the editor.
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => onSelect(c.name)}
        >
          <span className="font-[family-name:var(--font-mono)] text-fg">
            {sigil ? sigil + c.name.slice(1) : c.name}
          </span>
          <span className="text-[length:var(--fs-sm)] text-fg-muted">{c.description}</span>
        </button>
      ))}
    </div>
  );
}
