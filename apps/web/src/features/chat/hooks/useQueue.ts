import { useEffect, useMemo, useRef, useState } from "react";
import { api, type Message, type Thread } from "~/api";
import type { useComposerDraft } from "~/features/chat/composer/hooks/useComposerDraft";
import { errorText } from "~/lib/util";

/**
 * The messages waiting in a thread's queue for the agent, and what the queued strip does with
 * them: edit one in the composer, remove it, steer the running turn with it, or send the queue on.
 */
export function useQueue({
  thread,
  working,
  draft,
  setError,
}: {
  thread: Thread | null;
  working: boolean;
  draft: ReturnType<typeof useComposerDraft>;
  setError: (text: string | null) => void;
}) {
  const {
    draft: text,
    setDraft,
    attachments,
    setAttachments,
    queuedEdit,
    setQueuedEdit,
    composerDraft,
    composerAttachments,
    draftWithout,
  } = draft;
  const key = thread?.id ?? "new";
  const queued = useMemo(() => thread?.messages.filter((m) => m.queued) ?? [], [thread?.messages]);
  // Kept here so Ctrl+Enter in the composer can tell whether the queued strip is expanded.
  const [open, setOpen] = useState(true);
  // Ctrl+Enter shows the Steer button as pressed for a moment, as a mouse click would.
  const [steerPressed, setSteerPressed] = useState(false);
  const steerPressedTimer = useRef(0);

  // A queued message can be sent while it is being edited: the agent took it, or it was steered.
  // What was typed stays in the composer as an ordinary draft, to send or queue again, after the
  // draft the edit had set aside.
  useEffect(() => {
    if (!queuedEdit || queuedEdit.chat !== key || queued.some((m) => m.id === queuedEdit.id)) {
      return;
    }
    setDraft(text.trim() ? `${text}\n\n${queuedEdit.text}` : queuedEdit.text);
    setAttachments([
      ...attachments,
      ...queuedEdit.attachments.filter((a) => !attachments.some((c) => c.path === a.path)),
    ]);
    setQueuedEdit(null);
  }, [queued, queuedEdit, key]);

  // Editing puts a queued message's text and attachments in the composer.
  function edit(message: Message) {
    if (!thread) return;
    setQueuedEdit({
      chat: thread.id,
      id: message.id,
      text: message.text,
      attachments: draftWithout(message.text, message.attachments),
    });
  }

  // Takes a queued message back from the agent's queue.
  function remove(message: Message) {
    if (!thread) return;
    api.removeQueued(thread.id, message.id).catch((e: unknown) => setError(errorText(e)));
  }

  function editLatest() {
    if (queuedEdit || composerDraft.trim() || composerAttachments.length) return;
    const latest = queued.at(-1);
    if (latest) edit(latest);
  }

  function resume() {
    if (!thread || working) return;
    api.resumeQueued(thread.id).catch((e: unknown) => setError(errorText(e)));
  }

  function steer(message: Message) {
    if (!thread || !working) return;
    api.steerQueued(thread.id, message.id).catch((e: unknown) => setError(errorText(e)));
  }

  // Ctrl+Enter in the composer steers with the first queued message.
  function steerFirst() {
    if (!queued[0]) return;
    setSteerPressed(true);
    window.clearTimeout(steerPressedTimer.current);
    steerPressedTimer.current = window.setTimeout(() => setSteerPressed(false), 150);
    steer(queued[0]);
  }

  return {
    queued,
    open,
    toggle: () => setOpen((value) => !value),
    steerPressed,
    edit,
    remove,
    editLatest,
    resume,
    steer,
    steerFirst,
  };
}
