import { FolderIcon } from "@phosphor-icons/react";
import type { MentionMenu as Menu } from "~/features/chat/composer/hooks/useMentionMenu";
import type { MentionItem } from "~/features/chat/composer/types";

/** The project files and folders the @ being typed can mention, above the composer. */
export default function MentionMenu({
  menu,
  onPick,
}: {
  menu: Menu;
  onPick: (item: MentionItem) => void;
}) {
  return (
    <div
      role="listbox"
      aria-label="Files"
      className="absolute right-0 bottom-full left-0 mb-1.5 flex flex-col gap-px rounded-[var(--radius)] border border-border bg-panel p-1 shadow-[var(--shadow-popup)]"
    >
      {menu.items.length === 0 && (
        <div className="px-2 py-1.5 text-[length:var(--fs-sm)] text-fg-muted">
          No matching files
        </div>
      )}
      {menu.items.map((item, i) => (
        <button
          key={item.id}
          role="option"
          aria-selected={i === menu.index}
          className={`flex w-full cursor-pointer items-baseline rounded-[var(--radius)] border-none px-2 py-1.5 text-left font-[family-name:var(--font-mono)] text-[length:var(--fs-sm)] text-fg ${i === menu.index ? "bg-selected" : "bg-transparent hover:bg-hover"}`}
          // Keeps the focus in the editor.
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => onPick(item)}
        >
          {item.id.endsWith("/") && (
            <FolderIcon size={13} aria-hidden className="mr-1.5 shrink-0 self-center" />
          )}
          {item.label}
        </button>
      ))}
    </div>
  );
}
