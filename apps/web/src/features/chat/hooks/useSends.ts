import { useRef, useState } from "react";
import type { Attachment } from "~/api";
import type { useComposerDraft } from "~/features/chat/composer/hooks/useComposerDraft";
import type { NewThreadOptions } from "~/hooks/useThreads";
import { errorText } from "~/lib/util";

/** A message on its way to the agent, from the moment it is sent until the send settles. */
interface Send {
  id: number;
  /** The chat it goes to. A send from the new-task screen ("new") moves to its thread once that
   * is made, so the new thread shows as working until it answers. */
  chat: string;
  /** The chat whose composer holds the message, read-only, until the agent has taken it. */
  heldIn: string | null;
}

/**
 * The messages a chat sends. Each send is one item that knows the chat it goes to and the
 * composer it holds, so a chat is working or holding exactly while one of its sends is on its
 * way, and a send that settles takes both with it.
 */
export function useSends({
  chat,
  options,
  draft,
  onSend,
  onSent,
  setErrorFor,
}: {
  /** The open chat: its thread's id, or "new" for the new-task screen. */
  chat: string;
  /** What a send from the new-task screen starts its thread with. */
  options: NewThreadOptions;
  draft: ReturnType<typeof useComposerDraft>;
  onSend: (
    text: string,
    attachments: Attachment[],
    draft: NewThreadOptions,
    queue: boolean,
    onThread: (threadId: string) => void,
  ) => Promise<void>;
  /** Called once a send has gone through. */
  onSent: () => void;
  setErrorFor: (chat: string, text: string | null) => void;
}) {
  const [sends, setSends] = useState<Send[]>([]);
  const nextId = useRef(0);
  const { composerDraft, attachments, sentWith, draftWithout, restoreDraft, clearSent } = draft;

  async function dispatch(
    text: string,
    files: Attachment[],
    restoreOnError: boolean,
    queue = false,
    heldIn: string | null = null,
  ): Promise<{ sent: boolean; chat: string }> {
    const id = nextId.current++;
    let sentTo = chat;
    setErrorFor(chat, null);
    setSends((all) => [...all, { id, chat, heldIn }]);
    try {
      await onSend(text, files, options, queue, (threadId) => {
        sentTo = threadId;
        setSends((all) => all.map((s) => (s.id === id ? { ...s, chat: threadId } : s)));
      });
      onSent();
      return { sent: true, chat: sentTo };
    } catch (e) {
      // A send from the new-task screen has moved to its thread, if one was made. The error and
      // the text go back there.
      setErrorFor(sentTo, errorText(e));
      if (restoreOnError) restoreDraft(sentTo, text, draftWithout(text, files));
      return { sent: false, chat: sentTo };
    } finally {
      setSends((all) => all.filter((s) => s.id !== id));
    }
  }

  // Sends what the composer holds. The draft stays, read-only, until the agent has taken it: then
  // it empties, and a refusal leaves it as it was to send again.
  async function sendHeld(queue: boolean) {
    const from = chat;
    const [text, files] = [composerDraft, attachments];
    const message = composerDraft.trim();
    const { sent, chat: to } = await dispatch(
      message,
      sentWith(message, files),
      false,
      queue,
      from,
    );
    if (sent) {
      clearSent(from, files);
    } else if (to !== from) {
      // The chat the new task started holds it now.
      restoreDraft(to, text, files);
      clearSent(from, files);
    }
  }

  return {
    working: sends.some((s) => s.chat === chat),
    holding: sends.some((s) => s.heldIn === chat),
    dispatch,
    sendHeld,
  };
}
