import { useCallback, useSyncExternalStore } from "react";
import { api } from "~/api";

// The projects and folders that are collapsed, kept outside React state, and saved with the
// backend. A toggle wakes only the chevron and the collapsible region of the row concerned,
// instead of re-rendering every row in the tree, which made opening and closing a large project
// stutter. Folders are stored as `folder:<id>`, projects by their id.
let ids: ReadonlySet<string> = new Set();
// Each row's subscribers, by id, so a change reaches only the rows whose state changed.
const listeners = new Map<string, Set<() => void>>();
// Whole-set subscribers, for code that needs the tree as a whole after any change.
const allListeners = new Set<() => void>();

export function getCollapsed(): ReadonlySet<string> {
  return ids;
}

function apply(next: ReadonlySet<string>) {
  const changed = new Set<string>();
  for (const id of ids) if (!next.has(id)) changed.add(id);
  for (const id of next) if (!ids.has(id)) changed.add(id);
  ids = next;
  for (const id of changed) listeners.get(id)?.forEach((listener) => listener());
  if (changed.size > 0) allListeners.forEach((listener) => listener());
}

/** Calls `listener` after any project or folder opens or closes. Returns the unsubscribe. */
export function onCollapsedChange(listener: () => void): () => void {
  allListeners.add(listener);
  return () => void allListeners.delete(listener);
}

/** Changes which projects and folders are collapsed, and saves it. */
export function setCollapsed(next: ReadonlySet<string>) {
  apply(next);
  void api.setCollapsed([...next]);
}

/** Restores the saved set. Resolves once it is in place, or once loading failed, which keeps
 * everything expanded. */
export function loadCollapsed(): Promise<void> {
  return api.getCollapsed().then(
    (saved) => apply(new Set(saved)),
    () => {},
  );
}

/** Whether the project or folder `id` is collapsed; re-renders the caller only when that changes. */
export function useIsCollapsed(id: string): boolean {
  const subscribe = useCallback(
    (listener: () => void) => {
      const set = listeners.get(id) ?? new Set<() => void>();
      listeners.set(id, set);
      set.add(listener);
      return () => void set.delete(listener);
    },
    [id],
  );
  return useSyncExternalStore(subscribe, () => ids.has(id));
}
