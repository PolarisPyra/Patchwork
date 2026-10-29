import type { Editor } from "@tiptap/react";
import { DEFAULT_SIGILS, sigilKind } from "~/lib/sigils";

/** The "/word", "$word" or "&word" the caret is at the end of, at the start of a line or after a space. */
export interface CommandToken {
  /** What was typed, with the default sigil in place of the user's. */
  text: string;
  /** What was typed. */
  raw: string;
  from: number;
  to: number;
  /** The token is the whole draft, so a pick replaces the draft and can run a local command. */
  whole: boolean;
}

export function commandToken(editor: Editor): CommandToken | null {
  const { selection, doc } = editor.state;
  const { $from } = selection;
  if (!selection.empty || $from.parent.type.spec.code) return null;
  // Chips and mentions read as one character that ends the token.
  const before = $from.parent.textBetween(0, $from.parentOffset, undefined, "\ufffc");
  const match = /(?:^|\s)([^\s\ufffc]+)$/.exec(before);
  const kind = match && sigilKind(match[1][0]);
  if (!kind || kind === "files") return null;
  const raw = match[1];
  return {
    text: DEFAULT_SIGILS[kind] + raw.slice(1),
    raw,
    from: $from.pos - raw.length,
    to: $from.pos,
    whole: doc.childCount === 1 && doc.textContent === raw,
  };
}
