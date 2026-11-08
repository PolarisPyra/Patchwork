import { useSyncExternalStore } from "react";

/** Notes on lines of the diffs, kept by project until they go to the agent as one review. */

export interface ReviewNote {
  id: string;
  path: string;
  /** Where the lines are, as the diff numbers them: "lines 12–14", or "removed line 8". */
  where: string;
  /** The line the note shows under: its number in the new file, or in the old one for removed lines. */
  anchor: { side: "new" | "old"; line: number };
  /** The diff lines it is about, each with its +, - or space. */
  excerpt: string[];
  text: string;
}

/** Lines of a note's excerpt the review quotes; the rest are cut. */
const EXCERPT_LINES = 20;

const notes = new Map<string, ReviewNote[]>();
const listeners = new Set<() => void>();
const NONE: ReviewNote[] = [];

function set(projectId: string, list: ReviewNote[]) {
  if (list.length > 0) notes.set(projectId, list);
  else notes.delete(projectId);
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The project's notes, oldest first. */
export function useReview(projectId: string): ReviewNote[] {
  return useSyncExternalStore(subscribe, () => notes.get(projectId) ?? NONE);
}

export function addNote(projectId: string, note: Omit<ReviewNote, "id">) {
  set(projectId, [...(notes.get(projectId) ?? []), { ...note, id: crypto.randomUUID() }]);
}

export function editNote(projectId: string, id: string, text: string) {
  set(
    projectId,
    (notes.get(projectId) ?? []).map((n) => (n.id === id ? { ...n, text } : n)),
  );
}

export function removeNote(projectId: string, id: string) {
  set(
    projectId,
    (notes.get(projectId) ?? []).filter((n) => n.id !== id),
  );
}

export function clearReview(projectId: string) {
  set(projectId, []);
}

/** The notes as one message to the agent: each with its file, lines, and the lines it is about. */
export function reviewMessage(list: ReviewNote[]): string {
  const parts = list.map((note, i) => {
    const lines =
      note.excerpt.length > EXCERPT_LINES
        ? [...note.excerpt.slice(0, EXCERPT_LINES), "…"]
        : note.excerpt;
    // A fence longer than any run of backticks in the lines, so none of them ends it.
    const longest = Math.max(
      2,
      ...lines.map((l) => Math.max(0, ...(l.match(/`+/g) ?? []).map((r) => r.length))),
    );
    const fence = "`".repeat(longest + 1);
    return `${i + 1}. ${note.path}, ${note.where}:\n${fence}diff\n${lines.join("\n")}\n${fence}\n${note.text.trim()}`;
  });
  return `Review notes on your changes. Address each one:\n\n${parts.join("\n\n")}`;
}
