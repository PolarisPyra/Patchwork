import { useEffect, useState } from "react";
import { api, onApiProvidersChanged, type AgentKind, type ModelOption } from "~/api";

/** The models fetched per agent, shared by every composer. Each chat mounts its own composer, and
 * one that fetched afresh showed the pickers and the compact ring without the model's details
 * until the agent CLIs answered, and ran those CLIs again for every chat opened. */
const cached: Partial<Record<AgentKind, ModelOption[]>> = {};
/** The fetch in flight per agent, which composers that mount meanwhile share. */
const pending: Partial<Record<AgentKind, Promise<ModelOption[]>>> = {};

function fetchModels(kind: AgentKind): Promise<ModelOption[]> {
  pending[kind] ??= api
    .listModels(kind)
    .then((list) => (cached[kind] = list))
    .finally(() => delete pending[kind]);
  return pending[kind];
}

/** Models per agent, fetched once and kept, and read again when the window comes back. */
export function useModels(agents: AgentKind[]): Partial<Record<AgentKind, ModelOption[]>> {
  const [models, setModels] = useState<Partial<Record<AgentKind, ModelOption[]>>>(() => ({
    ...cached,
  }));
  const wanted = agents.join(",");
  useEffect(() => {
    let cancelled = false;
    const kinds = wanted.split(",") as AgentKind[];
    const load = (kind: AgentKind) =>
      fetchModels(kind).then(
        (list) => {
          if (!cancelled)
            setModels((prev) => (prev[kind] === list ? prev : { ...prev, [kind]: list }));
        },
        () => {},
      );
    for (const kind of kinds) {
      const list = cached[kind];
      if (!list) void load(kind);
      else setModels((prev) => (prev[kind] ? prev : { ...prev, [kind]: list }));
    }
    // An agent CLI can sign in to more models while the window is in the background.
    const onFocus = () => kinds.forEach(load);
    window.addEventListener("focus", onFocus);
    // API models change whenever a provider is edited in Settings.
    const stop = onApiProvidersChanged(() => {
      if (!kinds.includes("api")) return;
      api.listModels("api").then(
        (list) => {
          cached.api = list;
          if (!cancelled) setModels((prev) => ({ ...prev, api: list }));
        },
        () => {},
      );
    });
    return () => {
      cancelled = true;
      window.removeEventListener("focus", onFocus);
      stop();
    };
  }, [wanted]);
  return models;
}
