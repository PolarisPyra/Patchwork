/**
 * Pacing for reply text as it streams in.
 *
 * A model does not stream smoothly: deltas land a few times a second, each a few words long, so
 * text painted as it arrives moves in bursts. The reply is revealed at a steady rate instead, a
 * word edge at a time, chasing the arrived text but staying a beat behind it. The numbers below
 * are the ones Claude's web app uses: 22 characters a step, aiming to trail the arrived text by
 * about 550ms, with the wait between steps kept between 25ms and 150ms.
 */

/** The least text revealed in one step, in characters. */
export const STEP_CHARS = 22;
/** How far behind the arrived text the reveal is kept, in milliseconds. */
export const TARGET_LAG_MS = 550;
/** The shortest and longest wait between two steps. */
export const MIN_STEP_MS = 25;
export const MAX_STEP_MS = 150;
/** A word starts its fade after its offset in the step, up to a cap. */
export const WORD_DELAY_PER_CHAR_MS = 2.5;
export const WORD_DELAY_MAX_MS = 150;
/** Past this, a block is revealed without the word fade; the wrappers cost more than they show. */
export const ANIMATED_LIMIT = 20_000;

/** How much to reveal next, and how long to wait before revealing it. */
export interface RevealStep {
  /** Characters to reveal, at least `STEP_CHARS` and more when the reply has run ahead. */
  chars: number;
  /** Milliseconds to wait before the step. */
  delay: number;
}

/**
 * The next step for a reply that is `backlog` characters ahead of what is shown. A larger backlog
 * means bigger and quicker steps, so the reveal catches up rather than falling further behind.
 */
export function revealStep(backlog: number): RevealStep {
  const ahead = Math.max(0, backlog);
  const chars = Math.max(STEP_CHARS, Math.ceil(ahead / 4));
  const delay = Math.min(
    MAX_STEP_MS,
    Math.max(MIN_STEP_MS, (TARGET_LAG_MS * STEP_CHARS) / Math.max(1, ahead)),
  );
  return { chars, delay };
}

/** A space, or the end of a line. */
function isSpace(char: string): boolean {
  return /\s/.test(char);
}

/**
 * A character that is its own word in the scripts that break without spaces — Chinese, Japanese
 * kana, Korean. Cutting between two of them is a legal line break, so a step may stop there.
 */
function unspaced(char: string): boolean {
  return /[\u3040-\u30ff\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff\uac00-\ud7af]/.test(char);
}

/**
 * The index to reveal to, from `shown`, at least `chars` further on: the step finishes the word it
 * landed in and takes the space after it, so a word is never shown half-written. A word longer than
 * four steps, such as a pasted blob, is cut rather than revealed all at once, and a surrogate pair
 * is never split.
 */
export function revealEnd(text: string, shown: number, chars: number): number {
  const wanted = Math.min(text.length, shown + chars);
  if (wanted >= text.length) return text.length;
  const limit = Math.min(text.length, shown + chars * 4);
  let end = wanted;
  // Mid-word: run on to where the word ends, or to the limit for a word that is far too long.
  while (
    end < limit &&
    !isSpace(text[end - 1]) &&
    !isSpace(text[end]) &&
    !unspaced(text[end - 1]) &&
    !unspaced(text[end])
  ) {
    end++;
  }
  // Take the space after the word, so the next step starts on one.
  while (end < text.length && isSpace(text[end])) end++;
  // A code unit in the middle of a surrogate pair would render as a broken glyph.
  const last = text.charCodeAt(end - 1);
  if (end < text.length && last >= 0xd800 && last <= 0xdbff) end++;
  return end;
}

/** The fade delay for a word at `offset` characters into a step, in milliseconds. */
export function wordDelay(offset: number): number {
  return Math.min(Math.max(0, offset) * WORD_DELAY_PER_CHAR_MS, WORD_DELAY_MAX_MS);
}
