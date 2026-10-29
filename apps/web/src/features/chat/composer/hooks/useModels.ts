import { useEffect, useState } from "react";
import { api, onApiProvidersChanged, type AgentKind, type ModelOption } from "~/api";

/** Models per agent, fetched once per agent kind. */
export function useModels(agents: AgentKind[]): Partial<Record<AgentKind, ModelOption[]>> {
  const [models, setModels] = useState<Partial<Record<AgentKind, ModelOption[]>>>({});
  const wanted = agents.join(",");
  useEffect(() => {
    let cancelled = false;
    for (const kind of wanted.split(",") as AgentKind[]) {
      api.listModels(kind).then(
        (list) => {
          if (!cancelled) setModels((prev) => (prev[kind] ? prev : { ...prev, [kind]: list }));
        },
        () => {},
      );
    }
    // API models change whenever a provider is edited in Settings.
    const stop = onApiProvidersChanged(() => {
      if (!wanted.split(",").includes("api")) return;
      api.listModels("api").then(
        (list) => {
          if (!cancelled) setModels((prev) => ({ ...prev, api: list }));
        },
        () => {},
      );
    });
    return () => {
      cancelled = true;
      stop();
    };
  }, [wanted]);
  return models;
}
