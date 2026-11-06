import { useEffect, type RefObject } from "react";
import { type LegendListRef } from "@legendapp/list/react";
import { type Find } from "~/features/chat/find/matches";

/** The highlight names `styles/components.css` colors. */
const ALL = "chat-find";
const CURRENT = "chat-find-current";
/** How long after jumping to a row its match may still scroll into view as the row renders. */
const REVEAL_MS = 1_000;
/** Room kept above a revealed match, for the controls floating over the transcript's top. */
const TOP_ROOM = 80;

/** The ranges of `needle`, already lowercased, in the text under `element`, across its text nodes. */
function rangesIn(element: Element, needle: string): Range[] {
  const nodes: Text[] = [];
  const starts: number[] = [];
  let text = "";
  const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    starts.push(text.length);
    nodes.push(node as Text);
    text += (node as Text).data;
  }
  const point = (at: number): [Text, number] => {
    let i = nodes.length - 1;
    while (i > 0 && starts[i] > at) i--;
    return [nodes[i], at - starts[i]];
  };
  const ranges: Range[] = [];
  const haystack = text.toLowerCase();
  for (
    let at = haystack.indexOf(needle);
    at >= 0;
    at = haystack.indexOf(needle, at + needle.length)
  ) {
    const range = document.createRange();
    range.setStart(...point(at));
    range.setEnd(...point(at + needle.length));
    ranges.push(range);
  }
  return ranges;
}

/**
 * Highlights `find` in the transcript under `root` and keeps the current match in view.
 *
 * The list is virtual, so only the rows on screen are in the page: the highlights are painted
 * again, once a frame at most, whenever the transcript scrolls or its text changes. A new current
 * match is scrolled to once: straight to it when its row is drawn, or to its row first, after which
 * it is centered as the row renders.
 */
export function useFindHighlight(
  root: RefObject<HTMLElement | null>,
  list: RefObject<LegendListRef | null>,
  find: Find | undefined,
) {
  const query = find?.query.toLowerCase() ?? "";
  const id = find?.match?.id;
  const index = find?.match?.index;
  const nth = find?.match?.nth;

  useEffect(() => {
    const element = root.current;
    if (!element || !query.trim() || typeof Highlight === "undefined") return;
    let reveal: { jumpedAt: number | undefined } | undefined =
      id === undefined ? undefined : { jumpedAt: undefined };
    let frame = 0;

    const paint = () => {
      frame = 0;
      const all: Range[] = [];
      let current: Range | undefined;
      const seen = new Map<string, number>();
      for (const text of element.querySelectorAll<HTMLElement>("[data-find-text]")) {
        const owner = text.dataset.findText ?? "";
        for (const range of rangesIn(text, query)) {
          const n = seen.get(owner) ?? 0;
          seen.set(owner, n + 1);
          if (owner === id && n === nth) current = range;
          else all.push(range);
        }
      }
      CSS.highlights.set(ALL, new Highlight(...all));
      CSS.highlights.set(CURRENT, new Highlight(...(current ? [current] : [])));

      if (!reveal) return;
      const scroller = list.current?.getScrollableNode();
      if (current && scroller) {
        const view = scroller.getBoundingClientRect();
        const box = current.getBoundingClientRect();
        if (box.top < view.top + TOP_ROOM || box.bottom > view.bottom) {
          scroller.scrollBy({ top: box.top - (view.top + view.height / 2) });
        }
        reveal = undefined;
      } else if (reveal.jumpedAt === undefined && index !== undefined) {
        reveal.jumpedAt = performance.now();
        void list.current?.scrollToIndex({ index, viewOffset: TOP_ROOM });
      } else if (reveal.jumpedAt !== undefined && performance.now() - reveal.jumpedAt > REVEAL_MS) {
        // The match is not where the row's text was expected to have it, so the row is as close as it gets.
        reveal = undefined;
      }
    };
    const schedule = () => {
      if (!frame) frame = requestAnimationFrame(paint);
    };

    schedule();
    const observer = new MutationObserver(schedule);
    observer.observe(element, { childList: true, subtree: true, characterData: true });
    // Scroll events do not bubble, so the list's are caught on the way down.
    element.addEventListener("scroll", schedule, true);
    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
      element.removeEventListener("scroll", schedule, true);
      CSS.highlights.delete(ALL);
      CSS.highlights.delete(CURRENT);
    };
  }, [root, list, query, id, index, nth]);
}
