import {
  ArrowRightIcon,
  CaretRightIcon,
  ListBulletsIcon,
  PencilSimpleIcon,
  XIcon,
} from "@phosphor-icons/react";
import { useState } from "react";
import type { Message } from "~/api";
import { ComposerStripBody } from "~/components/chat/ComposerStrip";

const ICON_BUTTON =
  "grid h-6 w-6 flex-none cursor-pointer place-items-center rounded-[var(--radius-sm)] border-none bg-transparent text-fg-subtle hover:bg-hover hover:text-fg disabled:cursor-default disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-fg-subtle";

/**
 * Messages sent while the agent was busy, waiting for its turn to end. The agent has not seen
 * them yet, so each can be taken back to edit or dropped.
 */
export default function QueuedMessages({
  messages,
  canEdit,
  canSteer,
  onEdit,
  onRemove,
  onSteer,
  onResume,
  editingId,
  onCancelEdit,
}: {
  messages: Message[];
  /** Queued messages can be edited without replacing the current draft. */
  canEdit: boolean;
  canSteer: boolean;
  onEdit: (message: Message) => void;
  onRemove: (message: Message) => void;
  onSteer: (message: Message) => void;
  onResume: () => void;
  editingId: string | null;
  onCancelEdit: () => void;
}) {
  const [open, setOpen] = useState(true);
  if (messages.length === 0) return null;
  return (
    <div className="composer-strip mx-2 mb-1.5 overflow-hidden rounded-[var(--radius-sm)] border border-border-strong bg-panel text-[length:var(--fs-sm)] shadow-sm">
      <div className="flex h-9 min-w-0 items-center gap-2 bg-app px-2.5">
        <button
          type="button"
          aria-expanded={open}
          onClick={() => setOpen((value) => !value)}
          className="flex min-w-0 flex-1 cursor-pointer items-center gap-2 border-none bg-transparent p-0 text-left text-fg-muted hover:text-fg"
        >
          <ListBulletsIcon size={13} className="flex-none text-fg-subtle" />
          <span className="text-[length:var(--fs-xs)] font-semibold uppercase tracking-wide">
            Queued message
          </span>
          <CaretRightIcon
            size={12}
            className="composer-strip-caret ml-auto flex-none"
            data-open={open}
          />
        </button>
        <div
          key={`${editingId !== null}-${canSteer}`}
          className="composer-strip-state flex flex-none items-center gap-2"
        >
          {editingId !== null && (
            <button
              className="inline-flex h-6 cursor-pointer items-center justify-center rounded-[var(--radius-sm)] border border-border-strong bg-editor px-2.5 text-[length:var(--fs-xs)] text-fg hover:bg-hover"
              onClick={onCancelEdit}
            >
              Cancel edit
            </button>
          )}
          {!canSteer && (
            <button
              className="inline-flex h-6 cursor-pointer items-center justify-center rounded-[var(--radius-sm)] border border-border-strong bg-editor px-2.5 text-[length:var(--fs-xs)] text-fg hover:bg-hover"
              onClick={onResume}
            >
              Send
            </button>
          )}
        </div>
      </div>
      <ComposerStripBody open={open}>
        {messages.slice(0, 1).map((message) => (
          <div
            key={message.id}
            className="flex min-w-0 items-center gap-2 border-t border-border px-2.5 py-1.5"
          >
            <span
              className="min-w-0 flex-1 truncate text-fg"
              title={message.text || "Attachments only"}
            >
              {message.text || "Attachments only"}
            </span>
            <button
              className={ICON_BUTTON}
              onClick={() => onEdit(message)}
              disabled={!canEdit || editingId !== null}
              title={
                editingId === message.id ? "Editing this queued message" : "Edit queued message"
              }
              aria-label="Edit queued message"
            >
              <PencilSimpleIcon size={13} />
            </button>
            <button
              className="flex h-6 flex-none cursor-pointer items-center gap-1 rounded-[var(--radius-sm)] border border-transparent bg-transparent px-1.5 text-[length:var(--fs-xs)] text-accent-text hover:border-border hover:bg-hover disabled:cursor-default disabled:opacity-40 disabled:text-fg-subtle"
              onClick={() => onSteer(message)}
              disabled={!canSteer}
              title="Send this queued message as a steer"
              aria-label="Steer queued message"
            >
              <ArrowRightIcon size={13} /> Steer
            </button>
            <button
              className={ICON_BUTTON}
              onClick={() => onRemove(message)}
              title="Remove queued message"
              aria-label="Remove queued message"
            >
              <XIcon size={13} />
            </button>
          </div>
        ))}
      </ComposerStripBody>
    </div>
  );
}
