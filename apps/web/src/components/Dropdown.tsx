import { useRef, useState, type ReactNode } from "react";
import { useDismiss } from "~/hooks/useDismiss";

interface DropdownProps {
  trigger: (state: { open: boolean; toggle: () => void }) => ReactNode;
  /** Menu body; receives `close` so items can dismiss the menu. */
  children: (close: () => void) => ReactNode;
  /** Open above the trigger instead of below it. */
  up?: boolean;
  /** Line the menu's right edge up with the trigger's, for triggers near the right of a panel. */
  alignEnd?: boolean;
}

/** Minimal popup menu. Closes on outside click, on Escape, or when an item calls `close`. */
export default function Dropdown({
  trigger,
  children,
  up = false,
  alignEnd = false,
}: DropdownProps) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useDismiss(ref, open, () => setOpen(false));

  return (
    <div className="relative inline-flex min-w-0" ref={ref}>
      {trigger({ open, toggle: () => setOpen((o) => !o) })}
      {open && (
        <div
          className={`absolute z-20 flex min-w-[200px] flex-col rounded-[var(--radius)] border border-border-strong bg-elevated p-1 shadow-[var(--shadow-popup)] ${alignEnd ? "right-0" : "left-0"} ${up ? "bottom-[calc(100%_+_4px)]" : "top-[calc(100%_+_4px)]"}`}
          role="menu"
        >
          {children(() => setOpen(false))}
        </div>
      )}
    </div>
  );
}
