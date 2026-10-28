import { useSyncExternalStore } from "react";
import { api, type AgentsView } from "~/api";

/**
 * The custom agents, which the backend keeps up to date as their folder changes. One watch for
 * the whole window, started by the first reader, so the composer and Settings never ask again.
 */
const store = {
  view: { dir: "", agents: [] } as AgentsView,
  listeners: new Set<() => void>(),
  started: false,
  set(view: AgentsView) {
    this.view = view;
    this.listeners.forEach((listener) => listener());
  },
};

function start() {
  if (store.started) return;
  store.started = true;
  api
    .watchAgents((view) => store.set(view))
    .then(
      (view) => store.set(view),
      () => (store.started = false),
    );
}

const subscribe = (listener: () => void) => {
  start();
  store.listeners.add(listener);
  return () => void store.listeners.delete(listener);
};

/** The agents and their folder; re-renders when an agent is added, edited or removed. */
export function useAgents(): AgentsView {
  return useSyncExternalStore(subscribe, () => store.view);
}

/** Whether `name` is an agent, for the editor, which reads it outside React. */
export function isAgent(name: string): boolean {
  start();
  return store.view.agents.some((agent) => agent.name === name);
}
