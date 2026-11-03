import { CaretDownIcon, CaretRightIcon } from "@phosphor-icons/react";
import { type FoldSummary } from "~/features/chat/response/model";

/** The head line is a row that opens the folded work. As tall as the status line it replaces, so a
 * turn does not shift as it settles. */
const FOLD_CLASS =
  "flex h-6 cursor-pointer items-center gap-1.5 rounded-[var(--radius-sm)] border-none bg-transparent px-0 text-[length:var(--fs-md)] text-fg-muted hover:text-fg disabled:cursor-default disabled:hover:text-fg-muted";

/**
 * The head line of a settled turn: what it took, and a control for the steps it folded away. It
 * sits exactly where the running status line sat, so a turn does not lose a line as it finishes.
 */
export default function FoldHead({
  summary,
  open,
  onToggle,
}: {
  summary: FoldSummary;
  open: boolean;
  onToggle: () => void;
}) {
  const { label, action, superseded, waiting } = summary;
  const Caret = open ? CaretDownIcon : CaretRightIcon;
  const closable = action !== null;
  return (
    <div className="mb-1.5 flex flex-col items-start gap-1.5 border-b border-border pt-1 pb-2">
      <button
        type="button"
        aria-expanded={open}
        disabled={!closable}
        onClick={onToggle}
        // The gap between the label and the count is drawn, not written, so it is named here.
        aria-label={closable ? `${label}, ${action}` : undefined}
        className={`${FOLD_CLASS} tabular-nums`}
      >
        {/* Work still going sweeps the same band a running turn does. */}
        <span className={waiting ? "text-shimmer" : undefined}>{label}</span>
        {closable && (
          <>
            <span className="text-fg-subtle">{action}</span>
            <Caret size={13} />
          </>
        )}
      </button>
      {superseded && (
        <span className="text-[length:var(--fs-sm)] text-fg-subtle">Cut off by steer</span>
      )}
    </div>
  );
}
