import { useRef, useState } from "react";
import { DotsThreeIcon, PlugsIcon, XIcon } from "@phosphor-icons/react";
import { useDismiss } from "~/hooks/useDismiss";

/** The card's ⋯ menu: disconnect the agent session, or close the card. */
export default function LineageMenu({
  working,
  onDisconnect,
  onClose,
}: {
  working: boolean;
  onDisconnect: () => void;
  onClose: () => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useDismiss(ref, open, () => setOpen(false));

  return (
    <div className="relative" ref={ref}>
      <button
        type="button"
        className="grid h-6 w-6 flex-none cursor-pointer place-items-center rounded-[var(--radius)] border-none bg-transparent p-0 text-fg-subtle hover:bg-hover hover:text-fg"
        aria-label="Lineage actions"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        <DotsThreeIcon size={14} />
      </button>
      {open && (
        <div
          role="menu"
          className="absolute top-full right-0 z-20 mt-1 flex w-56 flex-col gap-px rounded-[var(--radius)] border border-border bg-panel p-1 shadow-[var(--shadow-popup)]"
        >
          <button
            type="button"
            role="menuitem"
            disabled={!working}
            className="flex w-full cursor-pointer items-center gap-2 rounded-[var(--radius)] border-none bg-transparent px-2 py-1.5 text-left text-fg hover:bg-hover disabled:cursor-default disabled:text-fg-subtle disabled:hover:bg-transparent"
            onClick={() => {
              setOpen(false);
              onDisconnect();
            }}
          >
            <PlugsIcon size={14} className="flex-none text-fg-muted" />
            Disconnect agent session
          </button>
          <button
            type="button"
            role="menuitem"
            className="flex w-full cursor-pointer items-center gap-2 rounded-[var(--radius)] border-none bg-transparent px-2 py-1.5 text-left text-fg hover:bg-hover"
            onClick={() => {
              setOpen(false);
              onClose();
            }}
          >
            <XIcon size={14} className="flex-none text-fg-muted" />
            Close lineage
          </button>
        </div>
      )}
    </div>
  );
}
