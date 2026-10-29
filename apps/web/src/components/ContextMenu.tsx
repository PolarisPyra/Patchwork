import { useEffect, useRef } from "react";
import type { Icon } from "@phosphor-icons/react";
import { useDismiss } from "~/hooks/useDismiss";

export interface MenuItem {
  label: string;
  Icon: Icon;
  danger?: boolean;
  onSelect: () => void;
}

/** Menu opened at a point, e.g. by right-clicking a row. Closes on outside click, Escape, or a choice. */
export default function ContextMenu({
  x,
  y,
  items,
  onClose,
}: {
  x: number;
  y: number;
  items: MenuItem[];
  onClose: () => void;
}) {
  const panel = useRef<HTMLDivElement>(null);

  useDismiss(panel, true, onClose);
  useEffect(() => {
    window.addEventListener("resize", onClose);
    return () => window.removeEventListener("resize", onClose);
  }, [onClose]);

  return (
    <div
      ref={panel}
      className="fixed z-20 flex min-w-[160px] flex-col rounded-[var(--radius)] border border-border-strong bg-elevated p-1 shadow-[var(--shadow-popup)]"
      role="menu"
      style={{ left: x, top: y }}
    >
      {items.map(({ label, Icon, danger, onSelect }) => (
        <button
          key={label}
          role="menuitem"
          className={`flex h-[26px] cursor-pointer items-center gap-1.5 rounded-[var(--radius-sm)] border-none bg-transparent px-2 text-left whitespace-nowrap hover:bg-hover ${danger ? "text-danger" : "text-fg"}`}
          onClick={() => {
            onClose();
            onSelect();
          }}
        >
          <Icon size={14} />
          {label}
        </button>
      ))}
    </div>
  );
}
