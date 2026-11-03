import type { RefObject } from "react";
import { ArrowUpIcon, CaretDownIcon, CheckIcon } from "@phosphor-icons/react";
import Spinner from "~/components/Spinner";

/**
 * Sends the draft. While a turn runs, it queues the message for after the turn or steers the turn,
 * and a menu beside it picks which; it updates a queued message being edited instead. The
 * composer closes the menu on a press outside `menuRef` or on Escape.
 */
export default function SendButton({
  working,
  editingQueued,
  sending,
  compacting,
  disabled,
  empty,
  overLimit,
  canQueueFollowUp,
  queueNext,
  menuOpen,
  menuRef,
  onMenuOpen,
  onChooseMode,
  onSend,
}: {
  working: boolean;
  editingQueued: boolean;
  sending: boolean;
  compacting: boolean;
  disabled: boolean;
  /** The draft has no text and no attachments. */
  empty: boolean;
  /** The draft is longer than a prompt can be. */
  overLimit: boolean;
  canQueueFollowUp: boolean;
  /** The next send queues rather than steers. */
  queueNext: boolean;
  menuOpen: boolean;
  menuRef: RefObject<HTMLDivElement | null>;
  onMenuOpen: (open: boolean) => void;
  onChooseMode: (queue: boolean) => void;
  onSend: () => void;
}) {
  const label = editingQueued
    ? "Update queued message"
    : sending
      ? "Sending"
      : compacting
        ? "Compacting context"
        : null;
  return (
    <div ref={menuRef} className="relative flex flex-none items-center">
      <button
        className={`grid h-8 w-8 cursor-pointer place-items-center ${working && !editingQueued ? "rounded-l-[var(--radius)]" : "rounded-[var(--radius)]"} border-none bg-accent text-on-accent enabled:hover:bg-accent-hover disabled:cursor-default disabled:bg-selected-muted disabled:text-fg-subtle`}
        onClick={onSend}
        disabled={
          disabled ||
          sending ||
          empty ||
          overLimit ||
          (compacting && !editingQueued) ||
          (working && queueNext && !canQueueFollowUp && !editingQueued)
        }
        title={
          label ??
          (working ? (queueNext ? "Queue for after the turn" : "Steer the running turn") : "Send")
        }
        aria-label={label ?? (working ? (queueNext ? "Queue message" : "Steer message") : "Send")}
      >
        {sending ? (
          <Spinner size={14} />
        ) : editingQueued ? (
          <CheckIcon size={16} />
        ) : (
          <ArrowUpIcon size={16} />
        )}
      </button>
      {working && !editingQueued && (
        <>
          <button
            type="button"
            className="grid h-8 w-6 cursor-pointer place-items-center rounded-r-[var(--radius)] border-0 border-l border-l-on-accent/20 bg-accent text-on-accent hover:bg-accent-hover"
            onClick={() => onMenuOpen(!menuOpen)}
            disabled={overLimit || empty}
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            aria-label="Choose queue or steer"
            title="Choose queue or steer"
          >
            <CaretDownIcon size={12} />
          </button>
          {menuOpen && (
            <div
              role="menu"
              aria-label="Send mode"
              className="absolute right-0 bottom-full z-50 mb-1.5 flex min-w-48 flex-col gap-px rounded-[var(--radius)] border border-border bg-panel p-1 shadow-[var(--shadow-popup)]"
            >
              <button
                disabled={overLimit}
                role="menuitemradio"
                aria-checked={!queueNext}
                className="w-full cursor-pointer rounded-[var(--radius-sm)] border-none bg-transparent px-2.5 py-2 text-left text-fg hover:bg-hover disabled:cursor-default disabled:opacity-40"
                onClick={() => onChooseMode(false)}
              >
                <span className="flex items-center gap-2">
                  <span className="grid w-3.5 flex-none place-items-center">
                    {!queueNext && <CheckIcon size={14} />}
                  </span>
                  Steer current turn
                </span>
                <span className="mt-0.5 block pl-[22px] text-[length:var(--fs-xs)] text-fg-subtle">
                  Send the next message into the active turn
                </span>
              </button>
              <button
                disabled={overLimit || !canQueueFollowUp}
                role="menuitemradio"
                aria-checked={queueNext}
                className="w-full cursor-pointer rounded-[var(--radius-sm)] border-none bg-transparent px-2.5 py-2 text-left text-fg hover:bg-hover disabled:cursor-default disabled:opacity-40"
                onClick={() => onChooseMode(true)}
              >
                <span className="flex items-center gap-2">
                  <span className="grid w-3.5 flex-none place-items-center">
                    {queueNext && <CheckIcon size={14} />}
                  </span>
                  Queue for next turn
                </span>
                <span className="mt-0.5 block pl-[22px] text-[length:var(--fs-xs)] text-fg-subtle">
                  {canQueueFollowUp
                    ? "Wait until the active turn finishes"
                    : "Edit or remove the queued message first"}
                </span>
              </button>
            </div>
          )}
        </>
      )}
    </div>
  );
}
