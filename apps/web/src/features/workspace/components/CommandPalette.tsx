import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { MagnifyingGlassIcon } from "@phosphor-icons/react";

export interface PaletteItem {
  id: string;
  group: string;
  label: string;
  hint?: string;
  run: () => void;
}

interface CommandPaletteProps {
  items: PaletteItem[];
  onClose: () => void;
}

interface GroupRows {
  name: string;
  rows: { item: PaletteItem; index: number }[];
}

export default function CommandPalette({ items, onClose }: CommandPaletteProps) {
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);

  const needle = query.trim().toLowerCase();
  const matches = items.filter((it) => it.label.toLowerCase().includes(needle));

  // Group in first-seen order. `index` numbers the rows in the order they show, so the arrow keys step
  // through the screen one row at a time, even when the items of one group are not next to each other.
  const groups: GroupRows[] = [];
  matches.forEach((item) => {
    let group = groups.find((g) => g.name === item.group);
    if (!group) {
      group = { name: item.group, rows: [] };
      groups.push(group);
    }
    group.rows.push({ item, index: 0 });
  });
  const filtered: PaletteItem[] = [];
  for (const group of groups)
    for (const row of group.rows) {
      row.index = filtered.length;
      filtered.push(row.item);
    }
  const active = Math.max(0, Math.min(index, filtered.length - 1));

  const listRef = useRef<HTMLDivElement>(null);
  const activeRow = useRef<HTMLButtonElement>(null);
  // True only after keyboard input, so hovering never scrolls the list.
  const scrollOnActive = useRef(false);
  // Where the pointer last was. WebKit sends a mousemove with an unchanged position when the list
  // scrolls under a resting pointer, which would move the highlight to whatever row lands there.
  const pointer = useRef<{ x: number; y: number } | null>(null);
  const pointerMoved = (e: { screenX: number; screenY: number }) => {
    const last = pointer.current;
    pointer.current = { x: e.screenX, y: e.screenY };
    return last !== null && (last.x !== e.screenX || last.y !== e.screenY);
  };

  useEffect(() => {
    if (!scrollOnActive.current) return;
    scrollOnActive.current = false;
    if (active === 0) {
      if (listRef.current) listRef.current.scrollTop = 0;
      return;
    }
    // Vertical only: scrollIntoView also scrolls sideways, which shifts the text.
    const list = listRef.current;
    const row = activeRow.current;
    if (!list || !row) return;
    // The first row of a group also brings its header into view. EDGE matches the list's p-2 padding,
    // so a row that stops at the edge keeps the same gap as the list's padding.
    const EDGE = 8;
    const group = row.parentElement;
    const isFirst = row.previousElementSibling === group?.firstElementChild;
    const target = isFirst && group ? group : row;
    const top = target.offsetTop - EDGE;
    const bottom = row.offsetTop + row.offsetHeight + EDGE;
    if (top < list.scrollTop) {
      list.scrollTop = top;
    } else if (bottom > list.scrollTop + list.clientHeight) {
      list.scrollTop = bottom - list.clientHeight;
    }
  }, [active, needle]);

  function choose(item: PaletteItem) {
    onClose();
    item.run();
  }

  function onKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      scrollOnActive.current = true;
      setIndex(Math.min(active + 1, filtered.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      scrollOnActive.current = true;
      setIndex(Math.max(active - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const item = filtered[active];
      if (item) choose(item);
    }
  }

  return (
    <div
      className="fixed inset-0 z-[90] flex items-start justify-center pt-[15vh]"
      onMouseDown={onClose}
    >
      <div
        className="flex max-h-[60vh] w-[600px] max-w-[calc(100vw_-_32px)] flex-col overflow-hidden rounded-[var(--radius-lg)] border border-border-strong bg-elevated shadow-[var(--shadow-popup)]"
        role="dialog"
        aria-label="Command palette"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="flex h-14 flex-none items-center gap-3 border-b border-border px-5 text-fg-subtle">
          <MagnifyingGlassIcon size={18} />
          <input
            className="h-full min-w-0 flex-1 bg-transparent text-fg text-[length:var(--fs-lg)] outline-none select-text border-none"
            autoFocus
            placeholder="Search actions, tasks and projects…"
            value={query}
            onChange={(e) => {
              scrollOnActive.current = true;
              setQuery(e.target.value);
              setIndex(0);
            }}
            onKeyDown={onKeyDown}
          />
        </div>
        <div
          ref={listRef}
          className="relative min-h-0 flex-1 overflow-y-auto overflow-x-hidden p-2 [&::-webkit-scrollbar]:w-[6px]"
        >
          {filtered.length === 0 && (
            <div className="px-3 py-6 text-center text-[length:var(--fs-md)] text-fg-subtle">
              No matches
            </div>
          )}
          {groups.map((group) => (
            <div key={group.name}>
              <div className="px-3 pt-3 pb-1.5 text-[length:var(--fs-xs)] font-medium tracking-[0.06em] text-fg-subtle uppercase">
                {group.name}
              </div>
              {group.rows.map(({ item, index: i }) => {
                const isActive = i === active;
                return (
                  <button
                    key={item.id}
                    type="button"
                    ref={isActive ? activeRow : undefined}
                    className={`flex h-9 w-full cursor-pointer items-center gap-3 rounded-[var(--radius)] border-none px-3 text-left text-[length:var(--fs-md)] text-fg ${isActive ? "bg-selected" : "bg-transparent hover:bg-hover"}`}
                    // mousemove, not mouseenter: rows scrolling under a resting pointer must not move the highlight.
                    onMouseMove={(e) => {
                      if (!pointerMoved(e)) return;
                      scrollOnActive.current = false;
                      setIndex(i);
                    }}
                    onClick={() => choose(item)}
                  >
                    <span className="min-w-0 flex-1 truncate">{item.label}</span>
                    {item.hint && (
                      <span
                        className={`flex-none max-w-[220px] truncate font-[family-name:var(--font-mono)] text-[length:var(--fs-xs)] ${isActive ? "text-fg-muted" : "text-fg-subtle"}`}
                      >
                        {item.hint}
                      </span>
                    )}
                  </button>
                );
              })}
            </div>
          ))}
        </div>
        <div className="flex h-9 flex-none items-center gap-4 border-t border-border px-4 text-[length:var(--fs-xs)] text-fg-subtle">
          <span>↑↓ navigate</span>
          <span>↵ run</span>
          <span>esc close</span>
        </div>
      </div>
    </div>
  );
}
