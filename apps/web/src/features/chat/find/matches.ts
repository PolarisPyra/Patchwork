import { type Message } from "~/api";
import { withoutImageMarkdown } from "~/features/chat/lib/composerExtensions";
import { stepsOf } from "~/features/chat/response/model";
import { rowKindOf } from "~/features/chat/transcript/rows";

/** One place the query was found. */
export interface Match {
  /** The message it is in. */
  id: string;
  /** That message's row in the transcript. */
  index: number;
  /** Which of the message's matches it is, counting from 0. */
  nth: number;
}

/** What the transcript is asked to find, and the match to show. */
export interface Find {
  query: string;
  match: Match | undefined;
}

/**
 * The texts of a message as the transcript draws them, in order: the user's text without its
 * images, or the text steps of a reply. The transcript marks the same texts with `data-find-text`,
 * so the nth match here is the nth one it highlights.
 */
function textsOf(message: Message): string[] {
  const kind = rowKindOf(message);
  if (kind === "user") return [withoutImageMarkdown(message.text)];
  if (kind !== "assistant") return [];
  return stepsOf(message).flatMap((step) => (step.kind === "text" ? [step.text] : []));
}

/** How many times `needle`, already lowercased, is in `text`, without overlaps. */
function countIn(text: string, needle: string): number {
  const haystack = text.toLowerCase();
  let count = 0;
  for (
    let at = haystack.indexOf(needle);
    at >= 0;
    at = haystack.indexOf(needle, at + needle.length)
  ) {
    count++;
  }
  return count;
}

/** Every match of `query` in the messages, in order, ignoring case. */
export function findMatches(messages: readonly Message[], query: string): Match[] {
  const needle = query.toLowerCase();
  if (!needle.trim()) return [];
  const matches: Match[] = [];
  messages.forEach((message, index) => {
    const count = textsOf(message).reduce((sum, text) => sum + countIn(text, needle), 0);
    for (let nth = 0; nth < count; nth++) matches.push({ id: message.id, index, nth });
  });
  return matches;
}
