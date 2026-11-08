import { forgetAgentStatus } from "~/api/agents";
import { invoke } from "~/api/invoke";
import type {
  AgentKind,
  ApiModel,
  ApiProvider,
  DiscoveredServer,
  ModelOption,
  ProviderHealth,
} from "~/api/types";

/** A saved provider, which never carries its key back to the UI. */
export interface ApiProviderView extends ApiProvider {
  hasKey: boolean;
}

/** What happens to a provider's stored key when it is saved. */
export type KeyChange = { action: "keep" } | { action: "set"; key: string } | { action: "clear" };

/** Each API provider's preset, name, logo, and where it runs, by id, as last listed; an API model
 * shows them. */
const apiProviders: Record<
  string,
  { preset: string; name: string; logo: string | null; local: boolean; baseUrl: string }
> = {};
const rememberPresets = <T extends ApiProvider>(providers: T[]) => {
  for (const p of providers)
    apiProviders[p.id] = {
      preset: p.preset,
      name: p.name,
      logo: p.logo,
      local: p.local,
      baseUrl: p.baseUrl,
    };
  return providers;
};

/** The provider of a `provider/model` id, as last listed. */
export function apiProviderOf(model: string | null | undefined) {
  return model ? apiProviders[model.split("/")[0]] : undefined;
}

const API_PROVIDERS_CHANGED = "patchwork-api-providers-changed";
const apiProvidersChanged = () => {
  forgetAgentStatus("api");
  window.dispatchEvent(new Event(API_PROVIDERS_CHANGED));
};

/** Calls `cb` whenever an API provider is added, changed, or removed; returns the unsubscribe. */
export function onApiProvidersChanged(cb: () => void): () => void {
  window.addEventListener(API_PROVIDERS_CHANGED, cb);
  return () => window.removeEventListener(API_PROVIDERS_CHANGED, cb);
}

/** The name of the API provider a `provider/model` id belongs to. */
export function apiProviderName(model: string | null | undefined): string | undefined {
  return apiProviderOf(model)?.name;
}

/** Where a local provider's model runs: "this computer", or the machine its server is on. */
export function apiModelPlace(model: string | null | undefined): string | undefined {
  const provider = apiProviderOf(model);
  if (!provider?.local) return undefined;
  try {
    const host = new URL(provider.baseUrl).hostname;
    return ["localhost", "127.0.0.1", "[::1]", "0.0.0.0"].includes(host) ? "this computer" : host;
  } catch {
    return undefined;
  }
}

export const modelsApi = {
  /** Models for an agent, starting with "Default" (the CLI's own choice). */
  listModels: async (kind: AgentKind) => {
    // API models show their provider's logo, so the presets are read alongside.
    if (kind === "api") await modelsApi.listApiProviders().catch(() => []);
    return invoke<ModelOption[]>("list_models", { kind });
  },
  listApiProviders: () => invoke<ApiProviderView[]>("list_api_providers").then(rememberPresets),
  /** Adds a provider (`id` empty) or replaces one; the key is stored apart from the provider. */
  saveApiProvider: (provider: ApiProvider, key: KeyChange) =>
    invoke<ApiProviderView>("save_api_provider", { provider, key }).then((p) => {
      rememberPresets([p]);
      apiProvidersChanged();
      return p;
    }),
  removeApiProvider: (id: string) =>
    invoke<void>("remove_api_provider", { id }).then(apiProvidersChanged),
  /** A provider's stored key, for the settings' eye button; null when it has none. */
  revealApiKey: (id: string) => invoke<string | null>("reveal_api_key", { id }),
  /** The endpoint's own model list, which also checks the URL and key. A key typed but not saved
   * goes in `key`; otherwise the saved provider's key is used. */
  fetchApiModels: (query: {
    baseUrl: string;
    headers: { name: string; value: string }[];
    key: string | null;
    keyEnv: string | null;
    providerId: string | null;
  }) => invoke<ApiModel[]>("fetch_api_models", { query }),
  /** A saved provider's live health: whether it answers and which models it serves right now. */
  probeApiProvider: (id: string) => invoke<ProviderHealth>("probe_api_provider", { id }),
  /** Local model servers found on well-known ports, offered to prefill the add-server form. */
  discoverInferenceServers: () => invoke<DiscoveredServer[]>("discover_inference_servers"),
  /** Finds where a saved server moved on its machine and points it there; returns the new URL. */
  relocateApiProvider: (id: string) =>
    invoke<string>("relocate_api_provider", { id }).then((url) => {
      apiProvidersChanged();
      return url;
    }),
  /** Adds the first local model server that answers, or reuses its saved provider, and returns
   * one of its models as `provider/model`. */
  useLocalModel: () =>
    invoke<string>("use_local_model").then(async (model) => {
      await modelsApi.listApiProviders().catch(() => []);
      apiProvidersChanged();
      return model;
    }),
};
