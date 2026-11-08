import { useEffect, useId, useRef } from "react";
import { useDismiss } from "~/hooks/useDismiss";

interface DeleteDialogProps {
  title: string;
  description: string;
  onDelete: () => void;
  onCancel: () => void;
}

/**
 * Confirms a permanent delete in the app's own dialog, in place of the native message box. Keep
 * has focus when it opens, so Enter keeps the chats; Escape and a press outside also keep them.
 */
export default function DeleteDialog({
  title,
  description,
  onDelete,
  onCancel,
}: DeleteDialogProps) {
  const panel = useRef<HTMLDivElement>(null);
  const keep = useRef<HTMLButtonElement>(null);
  const titleId = useId();
  const descriptionId = useId();
  useDismiss(panel, true, onCancel);

  useEffect(() => {
    keep.current?.focus();
  }, []);

  return (
    <div className="fixed inset-0 z-[100] flex items-center justify-center">
      <div
        ref={panel}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={descriptionId}
        className="flex w-[360px] max-w-[calc(100vw_-_32px)] flex-col overflow-hidden rounded-[var(--radius)] border border-border-strong bg-elevated shadow-[var(--shadow-popup)]"
      >
        <div className="flex flex-col px-5 pt-3 pb-2.5">
          <h2
            id={titleId}
            className="m-0 text-[length:var(--fs-lg)] leading-tight font-semibold text-fg"
          >
            {title}
          </h2>
          <p
            id={descriptionId}
            className="m-0 mt-1 text-[length:var(--fs-md)] leading-tight text-fg-muted"
          >
            {description}
          </p>
        </div>
        <div className="flex flex-none items-center justify-end gap-2 border-t border-border px-4 py-2">
          <button ref={keep} className="btn" onClick={onCancel}>
            Keep
          </button>
          <button className="btn btn-danger" onClick={onDelete}>
            Delete
          </button>
        </div>
      </div>
    </div>
  );
}
