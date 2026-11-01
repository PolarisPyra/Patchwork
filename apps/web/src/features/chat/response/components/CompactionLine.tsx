import { useState } from "react";
import { CaretDownIcon, CaretRightIcon } from "@phosphor-icons/react";
import Markdown from "~/features/chat/markdown/Markdown";
import Expander from "~/features/chat/response/components/Expander";

const ROW_CLASS =
  "flex w-full min-w-0 items-center gap-3 py-0.5 pb-2 text-[length:var(--fs-sm)] text-fg-subtle";

/**
 * Where the agent compacted its context: a line across the turn, which opens to the summary the
 * agent went on from when it reported one.
 */
export default function CompactionLine({
  label,
  summary,
  entering,
}: {
  label: string;
  summary: string | null;
  entering: boolean;
}) {
  const [open, setOpen] = useState(false);
  const enter = entering ? " timeline-enter" : "";
  if (!summary) {
    return (
      <div className={ROW_CLASS + enter}>
        {label}
        <span className="h-px flex-1 bg-border" />
      </div>
    );
  }
  const Caret = open ? CaretDownIcon : CaretRightIcon;
  return (
    <div className={enter.trim()}>
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        className={`group ${ROW_CLASS} cursor-pointer border-none bg-transparent px-0 text-left hover:text-fg`}
      >
        {label}
        <Caret size={12} className="-ms-2 flex-none opacity-0 group-hover:opacity-100" />
        <span className="h-px flex-1 bg-border" />
      </button>
      {open && (
        <Expander>
          <div className="mb-2 max-h-64 overflow-auto rounded-[var(--radius-sm)] bg-editor px-2 py-1.5 text-[length:var(--fs-sm)] text-fg-muted selectable">
            <Markdown text={summary} />
          </div>
        </Expander>
      )}
    </div>
  );
}
