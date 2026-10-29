import { useDeferredValue } from "react";
import Markdown from "~/components/chat/Markdown";
import { useSmoothText } from "~/hooks/useSmoothText";

/**
 * A reply's text as it streams in: the arrived text is revealed a word at a time rather than
 * jumping each time a delta lands, and words that have just arrived fade in.
 *
 * The reveal lives here, in the one row that is growing, so a step repaints this row alone; the
 * deferred value drops deltas that pile up behind a busy frame in favor of the newest one.
 */
export default function StreamedMarkdown({
  text,
  streaming,
}: {
  text: string;
  /** The reply is still arriving, so its text is revealed at a steady rate. */
  streaming: boolean;
}) {
  const arrived = useDeferredValue(text);
  // The reveal keeps draining after the turn ends, so the words still to appear fade in too.
  const shown = useSmoothText(streaming ? arrived : text, streaming);
  return <Markdown text={shown} streamed={streaming || shown !== (streaming ? arrived : text)} />;
}
