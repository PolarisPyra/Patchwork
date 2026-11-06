import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent,
  type RefObject,
} from "react";
import { type LegendListRef } from "@legendapp/list/react";
import { type Message } from "~/api";
import { turnsOf, type Turn } from "~/features/chat/transcript/rows";

/** Pixels between two strips, until the rail would pass its maximum height. */
const SPACING = 12;
/** Fewer turns than this need no rail. */
const MIN_TURNS = 2;

/**
 * The rail's side of the list: its turns, which of them are on screen, and scrolling to one.
 * `refresh` is called on every scroll and resize of a row, and reads the list once per frame.
 */
export function useTurnRail(list: RefObject<LegendListRef | null>, messages: readonly Message[]) {
  const turns = useMemo(() => turnsOf(messages), [messages]);
  const [strips] = useState(() => new Map<string, HTMLSpanElement>());
  const [current, setCurrent] = useState<number>();
  const frame = useRef(0);

  const refresh = useCallback(() => {
    if (frame.current) return;
    frame.current = requestAnimationFrame(() => {
      frame.current = 0;
      const state = list.current?.getState();
      if (!state || state.startBuffered < 0) return;
      // The rows of the buffer that reach into the viewport.
      const bottom = state.scroll + state.scrollLength;
      let first: number | undefined;
      let last = -1;
      for (let index = state.startBuffered; index <= state.endBuffered; index++) {
        const top = state.positionAtIndex(index);
        const size = state.sizeAtIndex(index);
        if (!Number.isFinite(top) || top >= bottom) continue;
        if (top + Math.max(1, Number.isFinite(size) ? size : 1) > state.scroll) {
          first ??= index;
          last = index;
        }
      }
      for (const turn of turns) {
        const strip = strips.get(turn.id);
        const inView = String(first !== undefined && turn.index >= first && turn.index <= last);
        // Unchanged strips are left alone, so a scroll tick does not restyle every one.
        if (strip && strip.dataset.inView !== inView) strip.dataset.inView = inView;
      }
      // The first turn that starts on screen, or else the one being read above it.
      let reading: number | undefined;
      if (first !== undefined) {
        for (const [n, turn] of turns.entries()) {
          if (turn.index >= first) {
            if (turn.index <= last) reading = n;
            break;
          }
          reading = n;
        }
      }
      setCurrent(reading);
    });
  }, [list, turns, strips]);

  useEffect(() => {
    refresh();
    return () => {
      cancelAnimationFrame(frame.current);
      frame.current = 0;
    };
  }, [refresh]);

  const select = useCallback(
    (turn: Turn) =>
      void list.current?.scrollToIndex({
        index: turn.index,
        animated: !matchMedia("(prefers-reduced-motion: reduce)").matches,
        viewOffset: 24,
      }),
    [list],
  );
  const wheel = useCallback(
    (deltaY: number) => list.current?.getScrollableNode().scrollBy({ top: deltaY }),
    [list],
  );

  return { turns, strips, current, refresh, select, wheel };
}

/** How far down the rail turn `index` of `count` sits, as a percentage. */
function topPercent(index: number, count: number): number {
  return count <= 1 ? 0 : (index / (count - 1)) * 100;
}

/** One line of text for a preview, with Markdown's marks taken out. */
function previewText(text: string | undefined): string | undefined {
  const plain = text
    ?.slice(0, 1_500)
    .replace(/```[\s\S]*?(```|$)/g, " ")
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/[`*_~#>|]/g, "")
    .replace(/\s+/g, " ")
    .trim();
  return plain || undefined;
}

/**
 * A strip per turn down the transcript's right edge, clear of its scrollbar. The strips of the turns on screen are lit, the
 * ones near the pointer widen, and the turn under it shows a preview of its message and reply.
 * Clicking a strip, or choosing one with the arrow keys and Enter, scrolls to that turn.
 *
 * Which strips are lit changes on every scroll tick, so `Transcript` sets their `data-in-view`
 * through `strips` rather than rendering the rail again.
 */
export default function TurnRail({
  turns,
  current,
  strips,
  onSelect,
  onWheel,
}: {
  turns: readonly Turn[];
  /** The turn being read, which the keyboard starts from. */
  current: number | undefined;
  strips: Map<string, HTMLSpanElement>;
  onSelect: (turn: Turn) => void;
  /** Scrolls the transcript, since the rail sits over it. */
  onWheel: (deltaY: number) => void;
}) {
  const [active, setActive] = useState<number>();
  if (turns.length < MIN_TURNS) return null;

  const shown = active !== undefined && active < turns.length ? active : undefined;
  const turn = shown === undefined ? undefined : turns[shown];
  const ask = previewText(turn?.ask);
  const answer = previewText(turn?.answer);
  const move = (delta: number) =>
    setActive((index) => Math.max(0, Math.min(turns.length - 1, (index ?? current ?? 0) + delta)));
  const indexAt = (event: MouseEvent<HTMLElement>) => {
    const rect = event.currentTarget.getBoundingClientRect();
    const progress = Math.max(0, Math.min(1, (event.clientY - rect.top) / rect.height));
    return Math.round(progress * (turns.length - 1));
  };

  return (
    <div
      className="absolute top-1/2 right-3 z-10 w-8 -translate-y-1/2 select-none"
      style={{ height: `min(${(turns.length - 1) * SPACING}px, calc(100% - 8rem))` }}
      onWheel={(event) => onWheel(event.deltaY)}
    >
      <button
        type="button"
        className="absolute inset-0 cursor-pointer border-none bg-transparent p-0 outline-none focus-visible:ring-2 focus-visible:ring-[var(--border-focus)]"
        aria-label={`Jump to turn ${(shown ?? current ?? 0) + 1} of ${turns.length}: ${previewText(turns[shown ?? current ?? 0]?.ask) ?? "message"}`}
        onMouseMove={(event) => setActive(indexAt(event))}
        onMouseLeave={() => setActive(undefined)}
        onMouseDown={(event) => event.preventDefault()}
        onFocus={() => setActive((index) => index ?? current ?? 0)}
        onBlur={() => setActive(undefined)}
        onClick={(event) => {
          // A click from the keyboard has no pointer position, so it takes the turn chosen.
          const index = event.detail === 0 ? shown : indexAt(event);
          const chosen = index === undefined ? undefined : turns[index];
          if (chosen) onSelect(chosen);
        }}
        onKeyDown={(event) => {
          const key = event.key;
          if (key === "ArrowDown") move(1);
          else if (key === "ArrowUp") move(-1);
          else if (key === "Home") setActive(0);
          else if (key === "End") setActive(turns.length - 1);
          else return;
          event.preventDefault();
        }}
      >
        {turns.map((t, index) => {
          const distance = shown === undefined ? undefined : Math.abs(index - shown);
          return (
            // Only transform and opacity change, since scrolling and hovering change them often.
            <span
              key={t.id}
              aria-hidden
              data-in-view="false"
              ref={(node) => {
                if (node) strips.set(t.id, node);
                else strips.delete(t.id);
              }}
              className={`pointer-events-none absolute right-0 h-[3px] w-8 origin-right -translate-y-1/2 rounded-full transition-transform duration-150 after:absolute after:inset-0 after:rounded-full after:bg-fg after:opacity-0 after:transition-opacity after:duration-150 data-[in-view=true]:after:opacity-90 ${distance === 0 ? "bg-fg-muted" : "bg-fg-subtle"} ${
                distance === 0
                  ? "scale-x-100"
                  : distance === 1
                    ? "scale-x-[0.667]"
                    : distance === 2
                      ? "scale-x-[0.417]"
                      : "scale-x-[0.333]"
              }`}
              style={{ top: `${topPercent(index, turns.length)}%` }}
            />
          );
        })}
      </button>
      {shown !== undefined && (
        <div
          aria-hidden
          className="pointer-events-none absolute right-10 w-96 rounded-[var(--radius-lg)] border border-border bg-elevated px-4 py-3 shadow-[var(--shadow-popup)]"
          style={{
            top: `${topPercent(shown, turns.length)}%`,
            transform: `translateY(${shown === 0 ? "0%" : shown === turns.length - 1 ? "-100%" : "-50%"})`,
          }}
        >
          <div className="line-clamp-2 text-[length:var(--fs-lg)] font-medium leading-snug text-fg">
            {ask ?? "Message"}
          </div>
          {answer && (
            <div className="mt-1.5 line-clamp-4 text-[length:var(--fs-md)] leading-relaxed text-fg-muted">
              {answer}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
