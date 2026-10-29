import { useEffect, type RefObject } from "react";
import { useStableCallback } from "~/hooks/useStableHandlers";

/**
 * Calls `close` on a press outside `ref` or on Escape, while `open`. The press is caught in the
 * capture phase, so a parent that stops mousedown (such as the settings dialog) still closes it.
 */
export function useDismiss(
  ref: RefObject<HTMLElement | null>,
  open: boolean,
  close: () => void,
): void {
  const onClose = useStableCallback(close);
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node | null)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("mousedown", onDown, true);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown, true);
      document.removeEventListener("keydown", onKey);
    };
  }, [open, ref, onClose]);
}
