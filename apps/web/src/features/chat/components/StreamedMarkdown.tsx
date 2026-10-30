import { useDeferredValue, useRef } from "react";
import Markdown from "~/features/chat/components/Markdown";
import { useSmoothText } from "~/features/chat/hooks/useSmoothText";
import { usePinnedToEnd } from "~/features/chat/hooks/usePinnedToEnd";
import { closeOpenMarks } from "~/features/chat/lib/streamReveal";

/**
 * A reply's text as it streams in: the arrived text is revealed a word at a time rather than
 * jumping each time a delta lands, and words that have just arrived fade in.
 *
 * The reveal lives here, in the one row that is growing, so a step repaints this row alone; the
 * deferred value drops deltas that pile up behind a busy frame in favor of the newest one. It is
 * also the row that keeps the transcript at its end as it grows: the list would otherwise move the
 * scroll a frame after the text it is meant to reveal, and the reply would bounce as it streams.
 */
export default function StreamedMarkdown({
  text,
  streaming,
  fresh = false,
}: {
  text: string;
  /** The reply is still arriving, so its text is revealed at a steady rate. */
  streaming: boolean;
  /** The text arrived while the reply was on screen, so it is revealed from its first word. */
  fresh?: boolean;
}) {
  const arrived = useDeferredValue(text);
  // The reveal keeps draining after the turn ends, so the words still to appear fade in too.
  const shown = useSmoothText(streaming ? arrived : text, streaming, fresh);
  const row = useRef<HTMLDivElement>(null);
  usePinnedToEnd(row);
  const streamed = streaming || shown.text !== (streaming ? arrived : text);
  return (
    <Markdown
      containerRef={row}
      text={streamed ? closeOpenMarks(shown.text) : shown.text}
      streamed={streamed}
      animate={shown.fade}
    />
  );
}
