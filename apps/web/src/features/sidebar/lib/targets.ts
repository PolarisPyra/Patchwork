import { type MouseEvent, type PointerEvent } from "react";
import { setSelectedRows } from "~/features/sidebar/lib/selectionState";
import type { Target } from "~/features/sidebar/types";

/**
 * How a row behaves when it is pressed: what counts as a plain press, which rows are picked, and
 * the keys that stand for a project or folder being open. Shared by every view of the sidebar.
 */

export const sameTarget = (a: Target, b: Target) => a.kind === b.kind && a.id === b.id;

/** The modifier keys a row click was made with. */
export type PickModifiers = Pick<MouseEvent, "ctrlKey" | "shiftKey">;

/**
 * A plain left press on a row, not on one of its buttons. It selects the row at once,
 * so a click that the browser swallows as a drag still selects.
 */
export const isPlainPress = (e: PointerEvent<HTMLElement>) =>
  e.button === 0 &&
  !e.ctrlKey &&
  !e.shiftKey &&
  !(e.target instanceof Element && e.target.closest("button"));

export const rowKey = (t: Target) => `${t.kind}:${t.id}`;

/** A click on the list's own blank space, not on a row or its label, clears the selection. */
export const clearSelectionOnBlank = (e: MouseEvent<HTMLElement>) => {
  if (e.target === e.currentTarget) setSelectedRows([]);
};

/** The collapsed-set key for a project or folder row. */
export const collapseKey = (t: Target) => (t.kind === "folder" ? `folder:${t.id}` : t.id);

/** A tree row's fill: the selection colors while the tree is focused, a hover tint otherwise. Each
 * row keeps 2px below it, so two filled rows next to each other do not run together. */
export const selectionFill = (selected: boolean) =>
  `mb-0.5 ${
    selected
      ? "bg-[var(--tree-inactive-selected,var(--bg-accent-soft))] hover:bg-[var(--tree-selected,var(--bg-selected))] group-focus-within/tree:bg-[var(--tree-selected,var(--bg-selected))] outline-transparent group-focus-within/tree:outline group-focus-within/tree:outline-1 group-focus-within/tree:-outline-offset-1 group-focus-within/tree:outline-[#3994bcb3]"
      : "bg-transparent hover:bg-[var(--tree-hover,var(--bg-hover))]"
  }`;
