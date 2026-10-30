import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { type Message } from "~/api";
import { tailOf } from "~/features/chat/transcript/lib/rows";

/** The part of the list's state this hook asks for, so it can be faked in a test. */
export interface ScrollList {
  getState(): {
    /** The list is showing its last row. */
    isAtEnd: boolean;
    /** Subscribes to whether the reader is close enough to the end for the list to keep following. */
    listen(
      key: "isWithinMaintainScrollAtEndThreshold",
      onChange: (within: boolean) => void,
    ): () => void;
  };
  scrollToEnd(options?: { animated?: boolean }): Promise<void> | void;
}

/**
 * How the transcript follows its own end: hiding the jump button while the list will follow on its
 * own, holding the first placement until the rows have been measured, and jumping to a turn the
 * moment it is sent.
 *
 * It lives here rather than in the list so the list stays a list, and it is written against
 * `ScrollList`, which is the three members it uses.
 */
export function useTranscriptScroll(
  list: React.RefObject<ScrollList | null>,
  messages: readonly Message[],
): { atEnd: boolean; settled: boolean } {
  const atEnd = useAtEnd(list);
  const settled = useSettledAtEnd(list);
  useFollowSentTurn(list, messages);
  return { atEnd, settled };
}

/**
 * Whether the reader is close enough to the end that the list keeps following it. It is the same
 * threshold the list uses, so the jump button only shows when the list will not follow on its own.
 */
function useAtEnd(list: React.RefObject<ScrollList | null>): boolean {
  const [atEnd, setAtEnd] = useState(true);
  useEffect(
    () => list.current?.getState().listen("isWithinMaintainScrollAtEndThreshold", setAtEnd),
    [list],
  );
  return atEnd;
}

/**
 * Whether the list has found its end. It first places rows by estimated height and only reaches
 * the end a frame or two later, once they are measured, so the transcript stays hidden until the
 * end has held for two frames: opening a chat does not show it jumping. A cap shows it anyway if
 * the end never settles.
 */
function useSettledAtEnd(list: React.RefObject<ScrollList | null>): boolean {
  const [settled, setSettled] = useState(false);
  useEffect(() => {
    let frame = 0;
    let held = 0;
    let frames = 0;
    const check = () => {
      // Measured rows can move the end after the first placement, so keep jumping to it until held.
      const atEnd = list.current?.getState().isAtEnd ?? false;
      if (!atEnd) void list.current?.scrollToEnd();
      held = atEnd ? held + 1 : 0;
      if (held >= 2 || ++frames > SETTLE_FRAME_CAP) setSettled(true);
      else frame = requestAnimationFrame(check);
    };
    frame = requestAnimationFrame(check);
    return () => cancelAnimationFrame(frame);
  }, [list]);
  return settled;
}

/** Frames the list may take to find its end before it is shown regardless. */
const SETTLE_FRAME_CAP = 30;

/**
 * Jump to a turn as it is sent, and to the reply that lands under it. On mount the list's own
 * `initialScrollAtEnd` has already done it, so nothing happens until the tail changes.
 */
function useFollowSentTurn(list: React.RefObject<ScrollList | null>, messages: readonly Message[]) {
  const { sent, reply } = tailOf(messages);
  const lastSent = useRef(sent);
  const lastReply = useRef(reply);
  useLayoutEffect(() => {
    if (lastSent.current === sent && lastReply.current === reply) return;
    lastSent.current = sent;
    lastReply.current = reply;
    // At the end, the list follows the new row itself. A jump started anyway was still under way
    // as the reply grew, and in a chat too short to scroll the list moved every row by that growth
    // for a frame.
    if (list.current?.getState().isAtEnd) return;
    void list.current?.scrollToEnd();
  }, [list, sent, reply]);
}
