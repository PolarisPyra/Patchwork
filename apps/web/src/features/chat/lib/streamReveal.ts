/**
 * Pacing for reply text as it streams in.
 *
 * A model does not stream smoothly: deltas land a few times a second, each a few words long, so
 * text painted as it arrives moves in bursts. The reply is revealed at a steady rate instead, a
 * word edge at a time, chasing the arrived text but staying a beat behind it.
 *
 * Three rules hold throughout: a step never ends inside a word, a step that carries a burst is
 * shown without the per-word fade, and no reveal may schedule itself past a hard horizon.
 */

/** The least text revealed in one step, in characters. */
export const STEP_CHARS = 22;
/** How far behind the arrived text the reveal is kept, in milliseconds. */
export const TARGET_LAG_MS = 550;
/** The shortest and longest wait between two steps. */
export const MIN_STEP_MS = 25;
export const MAX_STEP_MS = 150;
/**
 * The furthest ahead a step may be scheduled. A backlog is always cleared inside this, by taking
 * bigger steps, so a slow connection cannot leave the reveal falling further and further behind.
 */
export const REVEAL_HORIZON_MS = 600;
/** A word starts its fade after its offset in the step, up to a cap. */
export const WORD_DELAY_PER_CHAR_MS = 2.5;
export const WORD_DELAY_MAX_MS = 150;
/**
 * A step that reveals more characters than this is a burst rather than a step, and is shown
 * without the per-word fade: fading a dozen words one after another reads as a smear instead of
 * text arriving. Past this, the words stop staggering and the text lands.
 */
export const WORD_FADE_MAX_CHARS = 48;
/** Past this, a block is revealed without the word fade; the wrappers cost more than they show. */
export const ANIMATED_LIMIT = 20_000;
/**
 * Text arriving faster than this, in characters per millisecond (1600 characters a second), is
 * shown as it lands: a fade cannot be staged over text that is already being replaced.
 */
export const FAST_ARRIVAL_CHARS_PER_MS = 1.6;
/** How often the arrival rate is sampled, in milliseconds. */
export const SPEED_WINDOW_MS = 100;
/** How long a fast sample keeps the fast path on after text stops arriving quickly. */
export const FAST_HOLD_MS = 400;

/** How much to reveal next, and how long to wait before revealing it. */
export interface RevealStep {
  /** Characters to reveal, at least `STEP_CHARS` and more when the reply has run ahead. */
  chars: number;
  /** Milliseconds to wait before the step. */
  delay: number;
}

/**
 * The next step for a reply that is `backlog` characters ahead of what is shown. A larger backlog
 * means bigger and quicker steps, so the reveal catches up rather than falling further behind, and
 * the steps are made bigger again if the backlog would not clear within the horizon.
 */
export function revealStep(backlog: number): RevealStep {
  const ahead = Math.max(0, backlog);
  const chars = Math.max(STEP_CHARS, Math.ceil(ahead / 4));
  const delay = Math.min(
    MAX_STEP_MS,
    Math.max(MIN_STEP_MS, (TARGET_LAG_MS * STEP_CHARS) / Math.max(1, ahead)),
  );
  const steps = Math.ceil(ahead / chars);
  if (steps * delay <= REVEAL_HORIZON_MS) return { chars, delay };
  // More steps than the horizon allows at this delay: take fewer, larger ones instead.
  const fits = Math.max(1, Math.floor(REVEAL_HORIZON_MS / delay));
  return { chars: Math.max(chars, Math.ceil(ahead / fits)), delay };
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
 * four steps, such as a pasted blob, is cut rather than revealed all at once — the horizon applies
 * to prose, and a blob with no break in it would otherwise hold the reveal until it ends. A
 * surrogate pair is never split.
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

/** How fast the text is arriving, from samples taken as it grows. */
export interface Arrival {
  /** When `length` was sampled, in milliseconds on the same clock as `noteArrival`. */
  at: number;
  /** The text length at that sample. */
  length: number;
  /** When a sample last came in faster than `FAST_ARRIVAL_CHARS_PER_MS`; `-Infinity` if never. */
  fastAt: number;
  /** The text is arriving faster than a fade can be staged over it. */
  tooFast: boolean;
}

/** The first sample for text that is already `length` characters long. */
export function arrivalStart(length: number, now: number): Arrival {
  return { at: now, length, fastAt: -Infinity, tooFast: false };
}

/**
 * The arrival state after `length` characters of text have arrived at `now`. The rate is taken
 * over windows of at least `SPEED_WINDOW_MS`, so a burst of small deltas reads as one fast sample
 * rather than as several slow ones, and a fast sample keeps the fast path on for `FAST_HOLD_MS`
 * after the last one so the fade does not flip on and off between bursts.
 */
export function noteArrival(previous: Arrival, length: number, now: number): Arrival {
  if (now - previous.at < SPEED_WINDOW_MS) return previous;
  const added = length - previous.length;
  if (added / (now - previous.at) > FAST_ARRIVAL_CHARS_PER_MS) {
    return { at: now, length, fastAt: now, tooFast: true };
  }
  return {
    at: now,
    length,
    fastAt: previous.fastAt,
    tooFast: previous.tooFast && now - previous.fastAt < FAST_HOLD_MS,
  };
}

/**
 * Text still arriving, with a code span or bold mark it has opened but not yet closed closed at
 * its end. Without this the marks show as `` ` `` and `**` until their closing half arrives, then
 * the words they hold change style. A mark with nothing after it yet is left out instead. Only the
 * last paragraph can hold an open mark, and a fenced code block that is still open is left alone.
 */
export function closeOpenMarks(text: string): string {
  if ((text.match(/^ {0,3}(```|~~~)/gm)?.length ?? 0) % 2 === 1) return text;
  const start = text.lastIndexOf("\n\n") + 1;
  // A mark closes on the word before it: `**bold **` is not bold, and a reveal ends after a space.
  let out = text.trimEnd();
  const space = text.slice(out.length);
  const tail = () => out.slice(start);
  if ((tail().match(/`/g)?.length ?? 0) % 2 === 1) {
    out = out.endsWith("`") ? out.slice(0, -1) : `${out}\``;
  }
  // Asterisks inside code spans are not marks.
  if (
    (tail()
      .replace(/`[^`]*`/g, "")
      .match(/\*\*/g)?.length ?? 0) %
      2 ===
    1
  ) {
    out = out.endsWith("**") ? out.slice(0, -2) : `${out}**`;
  }
  return out + space;
}
