import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { CaretDownIcon, CaretUpIcon, MagnifyingGlassIcon, XIcon } from "@phosphor-icons/react";
import { type Message } from "~/api";
import { findMatches, type Find } from "~/features/chat/find/matches";
import { visibleMessages } from "~/features/chat/transcript/rows";
import { useCommand } from "~/lib/shortcuts";

const ICON_BUTTON =
  "grid h-6 w-6 shrink-0 cursor-pointer place-items-center rounded-[var(--radius-sm)] border-none bg-transparent p-0 text-fg-muted hover:bg-hover hover:text-fg disabled:cursor-default disabled:opacity-40 disabled:hover:bg-transparent";

/**
 * A search button over the chat that widens into a find field. It counts the matches in the
 * thread and steps through them with Enter, Shift+Enter or its arrows; the transcript highlights
 * what `onFind` reports and scrolls to the current match. Ctrl/Cmd+F opens it, and Esc closes it.
 */
export default function FindField({
  messages,
  onFind,
}: {
  messages: readonly Message[];
  onFind: (find: Find | undefined) => void;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [current, setCurrent] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const toggle = useRef<HTMLButtonElement>(null);

  const shown = useMemo(() => visibleMessages(messages), [messages]);
  const matches = useMemo(() => findMatches(shown, query), [shown, query]);
  const at = matches.length ? Math.min(current, matches.length - 1) : -1;
  const match = matches[at];

  // Reported by value, so a reply streaming in does not count as a new match to scroll to.
  const key = open && query ? [query, match?.id, match?.index, match?.nth].join("\n") : "";
  const latest = useRef<Find | undefined>(undefined);
  latest.current = key ? { query, match } : undefined;
  useEffect(() => onFind(latest.current), [key, onFind]);

  const openField = () => {
    setOpen(true);
    // Focused once it is drawn; the text is selected so typing replaces the last search.
    requestAnimationFrame(() => input.current?.select());
  };
  const close = () => {
    setOpen(false);
    toggle.current?.focus();
  };
  const step = (delta: number) => {
    if (!matches.length) return;
    setCurrent((at + delta + matches.length) % matches.length);
  };
  useCommand("chat.find", openField);

  return (
    <div
      role="search"
      aria-label="Find in chat"
      className={`flex h-7 items-center overflow-hidden rounded-[var(--radius)] border border-border bg-panel shadow-[0_1px_3px_rgba(0,0,0,0.12)] transition-[width] duration-200 ease-out motion-reduce:transition-none ${open ? "w-72" : "w-7"}`}
      onKeyDown={(event) => {
        if (event.key !== "Escape" || !open) return;
        // Esc closes only the field, not the file panel or a selection behind it.
        event.preventDefault();
        event.stopPropagation();
        close();
      }}
    >
      <button
        ref={toggle}
        type="button"
        className="grid h-full w-[26px] shrink-0 cursor-pointer place-items-center border-none bg-transparent p-0 text-fg-muted hover:text-fg"
        aria-label={open ? "Focus find field" : "Find in chat"}
        aria-expanded={open}
        onClick={openField}
      >
        <MagnifyingGlassIcon size={14} />
      </button>
      {open && (
        <div className="timeline-status-enter flex min-w-0 flex-1 items-center gap-0.5 pr-1">
          <input
            ref={input}
            className="h-full min-w-0 flex-1 border-none bg-transparent p-0 text-[length:var(--fs-sm)] text-fg outline-none"
            aria-label="Find in chat"
            placeholder="Find"
            spellCheck={false}
            value={query}
            onChange={(event) => {
              const next = event.target.value;
              setQuery(next);
              // The newest match first, since a chat is read from its end. A match that streams in
              // after it does not move it.
              setCurrent(Math.max(0, findMatches(shown, next).length - 1));
            }}
            onKeyDown={(event) => {
              if (event.key !== "Enter") return;
              event.preventDefault();
              step(event.shiftKey ? -1 : 1);
            }}
          />
          <span
            role="status"
            className="shrink-0 px-1 text-[length:var(--fs-xs)] text-fg-subtle tabular-nums"
          >
            {query ? (matches.length ? `${at + 1} of ${matches.length}` : "No results") : ""}
          </span>
          <IconButton label="Previous match" disabled={!matches.length} onClick={() => step(-1)}>
            <CaretUpIcon size={13} />
          </IconButton>
          <IconButton label="Next match" disabled={!matches.length} onClick={() => step(1)}>
            <CaretDownIcon size={13} />
          </IconButton>
          <IconButton label="Close find" onClick={close}>
            <XIcon size={13} />
          </IconButton>
        </div>
      )}
    </div>
  );
}

function IconButton({
  label,
  disabled,
  onClick,
  children,
}: {
  label: string;
  disabled?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      className={ICON_BUTTON}
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
    >
      {children}
    </button>
  );
}
