import { XIcon } from "@phosphor-icons/react";

/** The last error an action in the workspace hit, above the status bar until dismissed. */
export default function ErrorToast({
  error,
  onDismiss,
}: {
  error: string | null;
  onDismiss: () => void;
}) {
  if (!error) return null;
  return (
    <div
      className="fixed right-4 bottom-[calc(var(--statusbar-h)_+_12px)] z-[95] flex max-w-[420px] items-center gap-2 rounded-[var(--radius)] border border-danger bg-elevated py-2 pr-2 pl-3 text-[length:var(--fs-sm)] shadow-[var(--shadow-popup)]"
      role="alert"
    >
      <span>{error}</span>
      <button className="btn-icon" aria-label="Dismiss" onClick={onDismiss}>
        <XIcon size={14} />
      </button>
    </div>
  );
}
