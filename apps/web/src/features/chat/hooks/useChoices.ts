import { useEffect, useRef, useState } from "react";
import { type Thread } from "~/api";
import {
  dismissedChoiceIds,
  parseChoices,
  rememberDismissedChoice,
} from "~/features/chat/lib/choices";

/**
 * The choices an agent offers in its last finished reply. They are shown once: answering,
 * dismissing, or leaving the chat records the reply as done, so reopening it or restarting the
 * app does not bring the panel back.
 */
export function useChoices(
  thread: Thread | null,
  working: boolean,
  onAnswer: (option: string) => void,
) {
  const lastMessage = thread?.messages.at(-1);
  const source =
    !working && lastMessage?.role === "assistant" && lastMessage.status === "complete"
      ? lastMessage
      : null;
  const choices = source ? parseChoices(source.text) : null;
  const [dismissed, setDismissed] = useState<string | null>(null);

  const showChoices =
    choices !== null &&
    source !== null &&
    dismissed !== source.id &&
    !dismissedChoiceIds().has(source.id);

  const dismiss = (id: string) => {
    rememberDismissedChoice(id);
    setDismissed(id);
  };

  // Leaving the chat with its choices still open counts as dismissing them.
  const shownChoice = useRef<string | null>(null);
  useEffect(() => {
    shownChoice.current = showChoices && source ? source.id : null;
  });
  useEffect(
    () => () => {
      if (shownChoice.current) rememberDismissedChoice(shownChoice.current);
    },
    [thread?.id],
  );

  const answerChoice = (option: string) => {
    if (source) dismiss(source.id);
    onAnswer(option);
  };
  const dismissChoices = () => {
    if (source) dismiss(source.id);
  };

  return { choices, showChoices, answerChoice, dismissChoices };
}
