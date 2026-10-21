import { useSyncExternalStore } from "react";
import type { Target } from "./Sidebar";

// The rows picked in the sidebar, kept outside React state: a pick re-renders only the sidebar,
// not the open chat, and the shortcuts in Workspace read it when a key is pressed.
let rows: Target[] = [];
const listeners = new Set<() => void>();

export function getPicked(): Target[] {
  return rows;
}

export function setPickedRows(next: Target[] | ((prev: Target[]) => Target[])) {
  const value = typeof next === "function" ? next(rows) : next;
  if (value === rows) return;
  rows = value;
  listeners.forEach((listener) => listener());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function usePicked(): Target[] {
  return useSyncExternalStore(subscribe, getPicked);
}
