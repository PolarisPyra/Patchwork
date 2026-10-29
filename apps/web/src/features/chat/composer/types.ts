/** The shapes the composer shares with its hooks, helpers, and the chat around it. */

export interface HistoryEntry {
  id: string;
  text: string;
}

/**
 * For a drop at this point, in CSS pixels, or at the caret when null: null when it is not on the
 * composer, or else a function that writes the dropped paths into the draft where they were dropped.
 */
export type DropMentions = (
  point: {
    x: number;
    y: number;
  } | null,
) => ((mentions: MentionItem[]) => void) | null;

/** An entry in the @ menu, or a dropped path. */
export interface MentionItem {
  id: string;
  label: string;
  /** Goes in as the image, rather than a mention of its path. */
  image?: boolean;
}
