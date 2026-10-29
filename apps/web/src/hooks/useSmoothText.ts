import { useCallback, useEffect, useRef, useState } from "react";
import { revealEnd, revealStep } from "~/lib/streamReveal";

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

/**
 * The reply text to show, revealed a word at a time while it streams.
 *
 * `text` is everything that has arrived; the reply is shown a few characters behind it, at a rate
 * that rises with the backlog, so the browser paints steady steps instead of one jump per delta.
 * A turn that ends mid-reveal still drains at the same rate rather than dumping the rest at once.
 *
 * Text that was replaced rather than extended (an edited reply, a draft restored mid-reveal) is
 * shown whole at once, as is text the row is first given: opening a reply that is already streaming
 * shows what exists instead of replaying it. The system's reduced-motion setting turns the reveal
 * off.
 */
export function useSmoothText(text: string, streaming: boolean): string {
  const reducedMotion = usePrefersReducedMotion();
  const [shown, setShown] = useState(text);
  /** The arrived text, read by a step that is already waiting. */
  const target = useRef(text);
  /** The text on screen, which is always a prefix of `target`. */
  const current = useRef(text);
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
      return;
    }
    const { chars, delay } = revealStep(want.length - have.length);
    current.current = want.slice(0, revealEnd(want, have.length, chars));
    setShown(current.current);
    if (current.current.length < want.length) timer.current = window.setTimeout(tick, delay);
  }, []);

  useEffect(() => {
    target.current = text;
    // Nothing to pace: the reply was replaced rather than extended, or motion is turned off.
    if (reducedMotion || !text.startsWith(current.current)) {
      stop();
      current.current = text;
      setShown(text);
      return;
    }
    if (current.current.length >= text.length || timer.current !== null) return;
    timer.current = window.setTimeout(tick, revealStep(text.length - current.current.length).delay);
  }, [text, reducedMotion, stop, tick]);

  useEffect(() => stop, [stop]);

  // Behind the arrived text: still revealing. The reply ends before the reveal does, so a turn that
  // has finished keeps draining instead of jumping to its end.
  const draining = shown.length < text.length && text.startsWith(shown);
  if (reducedMotion) return text;
  return streaming || draining ? shown : text;
}
