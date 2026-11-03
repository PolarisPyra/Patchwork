import { useEffect, useLayoutEffect, useState, type RefObject } from "react";

/**
 * Keeps the picker beside its button: above it unless there is more room below, inside the
 * window, and following it as the window resizes or scrolls. A press outside closes it, and the
 * page behind does not scroll while it is open. Returns whether the Legacy models panel opens to
 * the left, for want of room on the right.
 */
export function usePopupPlacement(
  anchor: HTMLButtonElement,
  panel: RefObject<HTMLDivElement | null>,
  input: RefObject<HTMLInputElement | null>,
  close: () => void,
): boolean {
  const [flyoutLeft, setFlyoutLeft] = useState(false);

  useLayoutEffect(() => {
    const position = () => {
      if (!panel.current) return;
      const rect = anchor.getBoundingClientRect();
      const above = rect.top - 12;
      const below = window.innerHeight - rect.bottom - 12;
      const up = above >= 346 || above >= below;
      const left = Math.max(8, Math.min(rect.left, window.innerWidth - 368));
      Object.assign(panel.current.style, {
        left: `${left}px`,
        top: up ? "auto" : `${rect.bottom + 4}px`,
        bottom: up ? `${window.innerHeight - rect.top + 4}px` : "auto",
        maxHeight: `${Math.max(0, Math.min(346, up ? above : below))}px`,
      });
      setFlyoutLeft(left + 360 + 6 + 300 > window.innerWidth - 8);
    };
    position();
    input.current?.focus({ preventScroll: true });
    const observer = new ResizeObserver(position);
    observer.observe(anchor);
    window.addEventListener("resize", position);
    window.addEventListener("scroll", position, true);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", position);
      window.removeEventListener("scroll", position, true);
    };
  }, [anchor]);

  useEffect(() => {
    const outside = (event: PointerEvent) => {
      if (
        event.target instanceof Node &&
        !panel.current?.contains(event.target) &&
        !anchor.contains(event.target)
      )
        close();
    };
    const wheel = (event: WheelEvent | TouchEvent) => {
      if (event.target instanceof Node && !panel.current?.contains(event.target))
        event.preventDefault();
    };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("wheel", wheel, { passive: false });
    document.addEventListener("touchmove", wheel, { passive: false });
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("wheel", wheel);
      document.removeEventListener("touchmove", wheel);
    };
  }, [anchor, close]);

  return flyoutLeft;
}
