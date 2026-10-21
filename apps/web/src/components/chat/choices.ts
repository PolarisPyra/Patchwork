/** Fence language an agent uses to offer a choice: a JSON array of option labels. */
export const CHOICES_LANG = "choices";

/** The options of the last `choices` block in a reply, or null if there is none. */
export function parseChoices(text: string): string[] | null {
  const fences = [...text.matchAll(/```choices[ \t]*\n([\s\S]*?)\n```/g)];
  const last = fences.at(-1);
  if (!last) return null;
  try {
    const parsed: unknown = JSON.parse(last[1]);
    if (!Array.isArray(parsed)) return null;
    const options = parsed.filter((o): o is string => typeof o === "string" && o.trim() !== "");
    return options.length > 0 ? options.slice(0, 9) : null;
  } catch {
    return null;
  }
}

const DISMISSED_KEY = "patchwork.dismissedChoices";

/** Ids of the replies whose choices were answered, dismissed, or left unanswered. Kept across restarts. */
export function dismissedChoiceIds(): Set<string> {
  try {
    const ids: unknown = JSON.parse(localStorage.getItem(DISMISSED_KEY) ?? "[]");
    return new Set(Array.isArray(ids) ? ids.filter((id): id is string => typeof id === "string") : []);
  } catch {
    return new Set();
  }
}

/** Records that a reply's choices are done with, so reopening its thread does not show them again. */
export function rememberDismissedChoice(id: string) {
  const ids = [...dismissedChoiceIds(), id].slice(-200);
  localStorage.setItem(DISMISSED_KEY, JSON.stringify(ids));
}
