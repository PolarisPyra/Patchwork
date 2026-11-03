import { useEffect, useRef } from "react";
import type { Editor } from "@tiptap/react";

/** Where the caret goes as the chat opens, a turn ends, or the draft becomes writable again. */
export function useComposerFocus(
  editor: Editor | null,
  {
    deferFocus,
    disabled,
    sending,
    working,
  }: { deferFocus: boolean; disabled: boolean; sending: boolean; working: boolean },
) {
  // Opening a chat puts the caret in the composer, unless the user is typing in another field,
  // the task list keeps the focus (the chat opened from it), or a choice panel is open.
  const deferFocusRef = useRef(deferFocus || disabled);
  deferFocusRef.current = deferFocus || disabled;
  useEffect(() => {
    const active = document.activeElement;
    const focusStays =
      active instanceof HTMLElement &&
      (active.isContentEditable ||
        active.matches("input, textarea, select") ||
        active.closest("[data-keeps-focus]") !== null);
    if (editor && !focusStays && !deferFocusRef.current) editor.commands.focus("end");
  }, [editor]);

  // The Stop button holds focus once clicked, and it leaves the page when the turn or goal ends,
  // which drops focus to the body. Focus the editor then, unless the user moved elsewhere or the
  // reply offered choices.
  const wasWorking = useRef(working);
  useEffect(() => {
    if (
      wasWorking.current &&
      !working &&
      editor &&
      !deferFocus &&
      !disabled &&
      document.activeElement === document.body
    ) {
      editor.commands.focus("end");
    }
    wasWorking.current = working;
  }, [editor, working, deferFocus, disabled]);

  // A choice panel takes the keys: the editor is read-only and unfocused while it is open, and
  // takes the focus when it closes so the user can write their own answer. A disabled composer
  // is read-only the same way, and so is one whose message is being sent, which keeps the focus.
  // The caret comes back unless the user has put the focus somewhere else meanwhile.
  const readOnly = deferFocus || disabled || sending;
  const wasDeferred = useRef(readOnly);
  useEffect(() => {
    if (!editor) return;
    editor.setEditable(!readOnly, false);
    if (readOnly && !sending && editor.isFocused) editor.commands.blur();
    else if (!readOnly && wasDeferred.current) {
      const active = document.activeElement;
      if (active === document.body || active === editor.view.dom) editor.commands.focus();
    }
    wasDeferred.current = readOnly;
  }, [editor, readOnly, sending]);
}
