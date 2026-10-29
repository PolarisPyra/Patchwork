import { useCallback, useEffect, useRef, useState } from "react";
import {
  arrivalStart,
  noteArrival,
  revealEnd,
  revealStep,
  WORD_FADE_MAX_CHARS,
} from "~/features/chat/lib/streamReveal";

/** True when the system asks for less motion, which turns the paced reveal off. */
function usePrefersReducedMotion(): boolean {
  const [reduced, setReduced] = useState(
    () => window.matchMedia("(prefers-reduced-motion: reduce)").matches,
  );
  useEffect(() => {
    const query = window.matchMedia("(prefers-reduced-motion: reduce)");
    const onChange = () => setReduced(query.matches);
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);
  return reduced;
}

/** What to draw for a reply, and whether the words this render added should fade in. */
export interface SmoothText {
  /** The reply text to draw. */
  text: string;
  /** The words past the ones already on screen fade in; false turns that off for this render. */
  fade: boolean;
}

/**
 * The reply text to show, revealed a word at a time while it streams.
 *
 * `text` is everything that has arrived; the reply is shown a few characters behind it, at a rate
 * that rises with the backlog, so the browser paints steady steps instead of one jump per delta.
 * A turn that ends mid-reveal still drains at the same rate rather than dumping the rest at once.
 *
 * Three things turn the reveal off and show the text as it lands: the system's reduced-motion
 * setting, text that was replaced rather than extended (an edited reply, a draft restored
 * mid-reveal), and a stream arriving faster than a fade can be staged over it — past about 1600
 * characters a second, pacing it would only push the reply further behind. The fade, rather than
 * the reveal, is also dropped for a step that carries a burst of text.
 *
 * Text the row is first given is shown whole: opening a reply that is already streaming shows what
 * exists instead of replaying it.
 */
export function useSmoothText(text: string, streaming: boolean): SmoothText {
  const reducedMotion = usePrefersReducedMotion();
  const [shown, setShown] = useState(text);
  const [fade, setFade] = useState(true);
  /** The arrived text, read by a step that is already waiting. */
  const target = useRef(text);
  /** The text on screen, which is always a prefix of `target`. */
  const current = useRef(text);
  /** How fast the text is arriving, sampled as it grows. */
  const arrival = useRef(arrivalStart(text.length, performance.now()));
  const timer = useRef<number | null>(null);

  const stop = useCallback(() => {
    if (timer.current !== null) {
      clearTimeout(timer.current);
      timer.current = null;
    }
  }, []);

  // One step reads the latest text itself, so a delta that lands mid-wait is picked up by the step
  // already scheduled instead of restarting the wait and stalling the reveal.
  const tick = useCallback(() => {
    timer.current = null;
    const want = target.current;
    const have = current.current;
    if (!want.startsWith(have)) return;
    if (have.length >= want.length) {
      setShown(want);
      setFade(false);
      return;
    }
    const { chars, delay } = revealStep(want.length - have.length);
    current.current = want.slice(0, revealEnd(want, have.length, chars));
    const revealed = current.current.length - have.length;
    setShown(current.current);
    setFade(revealed <= WORD_FADE_MAX_CHARS);
    if (current.current.length < want.length) timer.current = window.setTimeout(tick, delay);
  }, []);

  useEffect(() => {
    target.current = text;
    arrival.current = noteArrival(arrival.current, text.length, performance.now());
    // Nothing to pace: the reply was replaced rather than extended, motion is turned off, or the
    // text is arriving faster than the reveal can follow.
    if (reducedMotion || arrival.current.tooFast || !text.startsWith(current.current)) {
      stop();
      current.current = text;
      setShown(text);
      setFade(false);
      return;
    }
    if (current.current.length >= text.length || timer.current !== null) return;
    timer.current = window.setTimeout(tick, revealStep(text.length - current.current.length).delay);
  }, [text, reducedMotion, stop, tick]);

  useEffect(() => stop, [stop]);

  if (reducedMotion) return { text, fade: false };
  // Behind the arrived text: still revealing. The reply ends before the reveal does, so a turn that
  // has finished keeps draining instead of jumping to its end.
  const draining = shown.length < text.length && text.startsWith(shown);
  if (!streaming && !draining) return { text, fade: false };
  return { text: shown, fade };
}
