import { memo, useMemo, useState } from "react";
import { ArrowCounterClockwiseIcon } from "@phosphor-icons/react";
import { type Message } from "~/api";
import AttachmentChip, { AttachmentTile } from "~/features/chat/AttachmentChip";
import { ACTION_CLASS } from "~/features/chat/markdown/CopyButton";
import { withoutImageMarkdown } from "~/features/chat/lib/composerExtensions";
import { formatClock } from "~/lib/util";

// Markdown loads with the chat, not on first use: its plain-text placeholder had another height,
// so swapping it in moved the messages.
import Markdown from "~/features/chat/markdown/Markdown";

/** User messages longer than this, such as pasted logs, show their first lines until expanded:
 * laying out all of one stalled the chat for over 100ms each time it scrolled into view. */
const LONG_MESSAGE_CHARS = 20000;
const PREVIEW_LINES = 40;
/** Long messages the user expanded; kept here because the chat list remounts rows as it scrolls. */
const expandedMessages = new Set<string>();

export const UserMessage = memo(function UserMessage({
  message,
  onRestore,
  onReveal,
}: {
  message: Message;
  /** Set when the message's turn can be undone. */
  onRestore?: (message: Message) => void;
  /** Shows the file an `@path` mention refers to in the file manager. */
  onReveal: (path: string) => void;
}) {
  // Images written into the text show above it with the other attachments.
  const full = useMemo(() => withoutImageMarkdown(message.text), [message.text]);
  const long = full.length > LONG_MESSAGE_CHARS;
  const [expanded, setExpanded] = useState(() => expandedMessages.has(message.id));
  const lines = useMemo(() => (long ? full.split("\n") : []), [long, full]);
  const text =
    long && !expanded && lines.length > PREVIEW_LINES
      ? lines.slice(0, PREVIEW_LINES).join("\n")
      : long && !expanded
        ? full.slice(0, LONG_MESSAGE_CHARS)
        : full;
  const toggle = () => {
    if (expanded) expandedMessages.delete(message.id);
    else expandedMessages.add(message.id);
    setExpanded(!expanded);
  };
  return (
    <div className="group flex max-w-[75%] flex-col items-end gap-1 self-end">
      <div className="flex items-center gap-1 px-2 text-[length:var(--fs-xs)] text-fg-subtle">
        {onRestore && (
          <button
            className={`${ACTION_CLASS} opacity-0 group-hover:opacity-100 focus-visible:opacity-100`}
            title="Put the project's files back as they were before this message"
            onClick={() => onRestore(message)}
          >
            <ArrowCounterClockwiseIcon size={13} /> Restore
          </button>
        )}
        <span>
          You · {formatClock(message.createdAt)}
          {message.steer && " · Steer"}
        </span>
      </div>
      {message.attachments.length > 0 && (
        <div className="flex flex-wrap justify-end gap-1.5">
          {message.attachments.map((a) =>
            a.kind === "image" ? (
              <AttachmentTile key={a.path} attachment={a} />
            ) : (
              <AttachmentChip key={a.path} attachment={a} />
            ),
          )}
        </div>
      )}
      {full && (
        // Line breaks the user typed are kept inside paragraphs only; between blocks they would show
        // as blank lines.
        <div
          className="rounded-[16px] bg-[color-mix(in_srgb,var(--text)_10%,var(--bg-editor))] px-4 py-2.5 leading-[1.55] text-fg [word-break:break-word] selectable [&_h3]:mt-0 [&_h3]:mb-0 [&_p]:whitespace-pre-wrap [&_[data-mention]]:cursor-pointer [&_[data-mention]]:hover:underline"
          onClick={(event) => {
            const path = (event.target as Element).closest<HTMLElement>("[data-mention]")?.dataset
              .mention;
            if (path) onReveal(path);
          }}
          onKeyDown={(event) => {
            if (event.key !== "Enter" && event.key !== " ") return;
            const path = (event.target as Element).closest<HTMLElement>("[data-mention]")?.dataset
              .mention;
            if (!path) return;
            event.preventDefault();
            onReveal(path);
          }}
        >
          {/* A goal or skill shows the chip the composer showed for it, in line with the text. */}
          <Markdown text={text} typed chips />
        </div>
      )}
      {long && (
        <button className={ACTION_CLASS} aria-expanded={expanded} onClick={toggle}>
          {expanded
            ? "Show less"
            : `Show full message (${Math.round(full.length / 1000)}k characters)`}
        </button>
      )}
    </div>
  );
});
