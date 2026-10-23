import { useRef, useState, type MouseEvent, type PointerEvent } from "react";

/** Drop-target marker and the class that draws the drop feedback on it. */
export interface DropTarget {
  attrs: { "data-drop": string };
  /** Class that marks this element as a drop target, or "" when it is not one. */
  edge: string;
}

/** Pointer handlers for one sidebar row that can be dragged and dropped on. */
export interface RowDrag extends DropTarget {
  attrs: DropTarget["attrs"] & {
    onPointerDown: (e: PointerEvent<HTMLElement>) => void;
    onClickCapture: (e: MouseEvent<HTMLElement>) => void;
  };
  dragging: boolean;
}

// A bar centred on the boundary between this row and its neighbour, so it sits in the gap
// above or below the row. The lists it sits in have 1px of padding, so its outer half is not clipped.
const LINE =
  "relative before:pointer-events-none before:absolute before:inset-x-0 before:z-10 before:h-0.5 before:bg-accent before:content-['']";
const LINE_ABOVE = `${LINE} before:-top-px`;
const LINE_BELOW = `${LINE} before:-bottom-px`;
const OUTLINE = "shadow-[inset_0_0_0_1px_var(--accent)]";

/** How far the pointer must move before a press becomes a drag. Shorter moves stay clicks. */
const DRAG_START_DISTANCE = 5;

interface Entry {
  kind: "row" | "folder";
  group: string;
  id: string;
  drop: (dragId: string, after: boolean) => void;
}

/**
 * Drag-to-reorder for sidebar rows, built on pointer events rather than the browser's native drag.
 * A native drag swallows the click of a press that moves a few pixels, so the row never opened.
 * Rows reorder within their own `group`, which can span folders (all of a project's tasks).
 * `onReorder` gets the destination list's ids in their new order, and the folder that list belongs to
 * (null: the top level).
 */
export function useRowReorder(
  onReorder: (group: string, ids: string[], folderId: string | null) => void,
) {
  const [drag, setDrag] = useState<{ group: string; id: string } | null>(null);
  const [target, setTarget] = useState<{ key: string; after: boolean } | null>(null);
  // Mirrors of the state above, for the window listeners, which outlive the render that created them.
  const dragRef = useRef<{ group: string; id: string } | null>(null);
  const targetRef = useRef<{ key: string; after: boolean } | null>(null);
  // Every drop target on screen, by its key, as of the last render.
  const entries = useRef(new Map<string, Entry>());
  const stopListening = useRef<(() => void) | null>(null);
  // A drag ends in a pointer release, which the browser also reports as a click. That click is ignored.
  const justDragged = useRef(false);

  const setTargetBoth = (next: { key: string; after: boolean } | null) => {
    targetRef.current = next;
    setTarget(next);
  };

  const finish = () => {
    stopListening.current?.();
    stopListening.current = null;
    // A press that never became a drag has nothing to clear; setting the state anyway re-renders
    // the whole tree on every click.
    const wasDragging = dragRef.current !== null;
    const hadTarget = targetRef.current !== null;
    dragRef.current = null;
    targetRef.current = null;
    if (wasDragging) setDrag(null);
    if (hadTarget) setTarget(null);
  };

  /** Starts tracking a press on a row. It turns into a drag once the pointer moves far enough. */
  const press = (e: PointerEvent<HTMLElement>, group: string, id: string) => {
    // Inline rename fields and the row's buttons keep their own presses.
    if (e.button !== 0 || e.target instanceof HTMLInputElement) return;
    const startX = e.clientX;
    const startY = e.clientY;

    const move = (ev: globalThis.PointerEvent) => {
      if (!dragRef.current) {
        if (Math.hypot(ev.clientX - startX, ev.clientY - startY) < DRAG_START_DISTANCE) return;
        dragRef.current = { group, id };
        setDrag({ group, id });
      }
      const hit = document.elementFromPoint(ev.clientX, ev.clientY)?.closest<HTMLElement>("[data-drop]");
      const key = hit?.dataset.drop;
      const entry = key !== undefined ? entries.current.get(key) : undefined;
      if (!hit || key === undefined || !entry || entry.group !== group || entry.id === id) {
        if (targetRef.current !== null) setTargetBoth(null);
        return;
      }
      // Above or below a row by which half the pointer is in; a folder takes the row inside it.
      const box = hit.getBoundingClientRect();
      const after = entry.kind === "row" && ev.clientY > box.top + box.height / 2;
      const current = targetRef.current;
      if (current?.key !== key || current.after !== after) setTargetBoth({ key, after });
    };

    const release = () => {
      const dragged = dragRef.current;
      const current = targetRef.current;
      const entry = current ? entries.current.get(current.key) : undefined;
      // Only a release over a drop target counts as a drop. Anywhere else, the press was a click.
      if (dragged && current && entry) {
        justDragged.current = true;
        setTimeout(() => {
          justDragged.current = false;
        }, 0);
        entry.drop(dragged.id, current.after);
      }
      finish();
    };

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", release);
    window.addEventListener("pointercancel", finish);
    stopListening.current = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", release);
      window.removeEventListener("pointercancel", finish);
    };
  };

  /** Handlers for a row. A drop puts the dragged row beside this one, in this row's list. */
  const row = (group: string, id: string, ids: string[], folderId: string | null): RowDrag => {
    const key = `row:${id}`;
    entries.current.set(key, {
      kind: "row",
      group,
      id,
      drop: (dragId, after) => {
        const rest = ids.filter((other) => other !== dragId);
        rest.splice(rest.indexOf(id) + (after ? 1 : 0), 0, dragId);
        onReorder(group, rest, folderId);
      },
    });
    return {
      attrs: {
        "data-drop": key,
        onPointerDown: (e) => press(e, group, id),
        onClickCapture: (e) => {
          if (justDragged.current) {
            e.preventDefault();
            e.stopPropagation();
          }
        },
      },
      edge: target?.key === key ? (target.after ? LINE_BELOW : LINE_ABOVE) : "",
      dragging: drag?.id === id,
    };
  };

  /** Drop target for a folder header. A drop appends the dragged row to the folder's list `ids`. */
  const folder = (group: string, key: string, ids: string[], folderId: string): DropTarget => {
    entries.current.set(key, {
      kind: "folder",
      group,
      id: folderId,
      drop: (dragId) => onReorder(group, [...ids.filter((other) => other !== dragId), dragId], folderId),
    });
    return { attrs: { "data-drop": key }, edge: target?.key === key ? OUTLINE : "" };
  };

  return { row, folder };
}
