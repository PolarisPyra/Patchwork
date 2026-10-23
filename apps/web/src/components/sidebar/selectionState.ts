import { useCallback, useSyncExternalStore } from "react";
import type { Target } from "./Sidebar";

// The sidebar's selection: the chats, tasks, folders and projects picked by click, Ctrl+click,
// Shift+click, Ctrl+A or the arrow keys, which bulk actions such as Delete act on. It is kept outside
// React state: a change re-renders only the sidebar, not the open chat, and the shortcuts in
// Workspace read it when a key is pressed.
let rows: Target[] = [];
const listeners = new Set<() => void>();
// Subscribers to one row's picked state, by row, so a pick wakes only the rows it changes and the
// tree around them is left alone. That keeps a click cheap in a tree with hundreds of tasks.
const rowListeners = new Map<string, Set<() => void>>();

const keyOf = (t: Target) => `${t.kind}:${t.id}`;

export function getSelectedRows(): Target[] {
  return rows;
}

export function setSelectedRows(next: Target[] | ((prev: Target[]) => Target[])) {
  const value = typeof next === "function" ? next(rows) : next;
  if (
    value === rows ||
    (value.length === rows.length &&
      value.every((t, i) => t.kind === rows[i].kind && t.id === rows[i].id))
  )
    return;
  const before = new Set(rows.map(keyOf));
  const after = new Set(value.map(keyOf));
  rows = value;
  listeners.forEach((listener) => listener());
  for (const key of before) if (!after.has(key)) rowListeners.get(key)?.forEach((l) => l());
  for (const key of after) if (!before.has(key)) rowListeners.get(key)?.forEach((l) => l());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useSelectedRows(): Target[] {
  return useSyncExternalStore(subscribe, getSelectedRows);
}

/** Whether `target` is selected; re-renders the caller only when that changes. */
export function useIsRowSelected(target: Target): boolean {
  const key = keyOf(target);
  const subscribeRow = useCallback(
    (listener: () => void) => {
      const set = rowListeners.get(key) ?? new Set<() => void>();
      rowListeners.set(key, set);
      set.add(listener);
      return () => void set.delete(listener);
    },
    [key],
  );
  return useSyncExternalStore(subscribeRow, () =>
    rows.some((t) => t.kind === target.kind && t.id === target.id),
  );
}
