import { useLayoutEffect, useRef, type RefObject } from "react";

/**
 * How far from the end the reader may be and still count as reading the end, in pixels. A layout
 * can leave a fraction behind, and a sub-pixel scroll must not read as having scrolled away.
 */
const END_SLACK_PX = 2;

/** How far the scroller is from its end, in pixels; 0 or less when it is at the end. */
function distanceFromEnd(scroller: HTMLElement): number {
  return scroller.scrollHeight - scroller.clientHeight - scroller.scrollTop;
}

/** The nearest ancestor of `node` that scrolls: the list this row sits in. */
function scrollParent(node: HTMLElement | null): HTMLElement | null {
  for (let el = node?.parentElement ?? null; el !== null; el = el.parentElement) {
    const { overflowY } = getComputedStyle(el);
    if (overflowY === "auto" || overflowY === "scroll") return el;
  }
  return null;
}

/**
 * Keeps the list at its end while the row that calls this grows.
 *
 * The transcript's list follows its end from a ResizeObserver: the row is measured, the list
 * re-renders, and the scroll happens in an animation frame after that. So the words a reply gains
 * are painted once below the end of the list, and the list jumps down to them a frame or two later —
 * the reply bounces up and down for as long as it streams. Correcting the scroll here, in a layout
 * effect, puts the growth and the scroll in the same frame, so a word arriving moves nothing.
 *
 * It only pulls the list down while the reader is at the end: their place is taken from the
 * scroller's own scroll events, so scrolling up during a reply leaves the list where they put it,
 * and returning to the end pins it again.
 */
export function usePinnedToEnd(row: RefObject<HTMLElement | null>): void {
  const scroller = useRef<HTMLElement | null>(null);
  /** How far from the end the reader was last seen, or null before the list is known. */
  const fromEnd = useRef<number | null>(null);

  // Find the list this row scrolls in, once, and note where the reader is as they move.
  useLayoutEffect(() => {
    const found = scrollParent(row.current);
    if (found === null) return;
    const note = () => {
      fromEnd.current = distanceFromEnd(found);
    };
    scroller.current = found;
    note();
    found.addEventListener("scroll", note, { passive: true });
    return () => {
      found.removeEventListener("scroll", note);
      scroller.current = null;
      fromEnd.current = null;
    };
  }, [row]);

  // Every commit: a row that grew takes the list's end with it, before the growth is painted.
  useLayoutEffect(() => {
    const list = scroller.current;
    if (list === null || fromEnd.current === null || fromEnd.current > END_SLACK_PX) return;
    const end = list.scrollHeight - list.clientHeight;
    if (list.scrollTop < end) list.scrollTop = end;
    fromEnd.current = 0;
  });
}
