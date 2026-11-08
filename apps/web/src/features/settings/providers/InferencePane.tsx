import { useCallback, useEffect, useState } from "react";
import { ArrowClockwiseIcon, MagnifyingGlassIcon } from "@phosphor-icons/react";
import {
  api,
  type ApiProvider,
  type ApiProviderView,
  type DiscoveredServer,
  type ProviderHealth,
} from "~/api";
import ProviderLogos from "~/components/ProviderLogos";
import Spinner from "~/components/Spinner";
import { blank, PROVIDER_TILE, TILE_GRID } from "~/features/settings/providers/ApiProvidersPane";
import { PromptEditor } from "~/features/settings/providers/ProvidersPane";
import { errorText } from "~/lib/util";

/** The probe of each server, by provider id: `undefined` while it is still checking. */
type HealthMap = Partial<Record<string, ProviderHealth>>;

// Kept across mounts so reopening Settings → Providers shows each server's last status at once,
// then refreshes it in place, instead of flashing every row back to "Checking…".
const healthCache = new Map<string, ProviderHealth>();

/** The preset a found server is saved as, which picks its logo: only LM Studio has one. */
const presetOf = (d: DiscoveredServer) => (d.name === "LM Studio" ? "lmstudio" : "custom");

/** A custom provider prefilled from a server found on this computer, so Add opens the editor ready
 * to save. */
function fromDiscovered(d: DiscoveredServer): ApiProvider {
  return {
    ...blank({ id: "custom", name: "", baseUrl: "", models: [] }),
    local: true,
    preset: presetOf(d),
    name: d.name,
    baseUrl: d.baseUrl,
    models: d.models.map((id) => ({ id, name: null })),
  };
}

/** One line under a server's name: what it serves right now, or why it is not answering. */
function healthLine(p: ApiProviderView, health: ProviderHealth | undefined): string {
  if (health === undefined) return "Checking…";
  if (!health.reachable) return health.error ?? "Not answering";
  const served = health.servedModels.length;
  const configured = new Set(health.servedModels);
  const missing = p.models.filter((m) => !configured.has(m.id)).length;
  const parts = [`${served} model${served === 1 ? "" : "s"} served`];
  if (missing > 0) parts.push(`${missing} configured not served`);
  const reads = Object.values(health.images);
  const vision = reads.filter(Boolean).length;
  if (reads.length > 0)
    parts.push(
      vision === 0
        ? "text only"
        : vision === reads.length
          ? "reads images"
          : `${vision} of ${reads.length} read images`,
    );
  if (health.latencyMs !== null) parts.push(`${health.latencyMs} ms`);
  return parts.join(" · ");
}

/** A saved inference server: its live status, with Edit, its system prompt, and the use switch. */
function ServerRow({
  provider: p,
  health,
  prompting,
  onRecheck,
  onEdit,
  onPrompt,
  onToggle,
}: {
  provider: ApiProviderView;
  health: ProviderHealth | undefined;
  prompting: boolean;
  onRecheck: () => void;
  onEdit: () => void;
  onPrompt: () => void;
  onToggle: () => void;
}) {
  const status = health === undefined ? "Checking…" : health.reachable ? "Ready" : "Unreachable";
  return (
    <div className="flex items-center gap-3 border-b border-border py-2.5">
      <ProviderLogos kind="api" preset={p.preset} logo={p.logo} size={20} />
      <button
        className="min-w-0 flex-1 cursor-pointer border-none bg-transparent p-0 text-left text-fg"
        onClick={onEdit}
      >
        <div className="font-semibold">{p.name}</div>
        <div className="truncate text-[length:var(--fs-sm)] text-fg-muted">
          {healthLine(p, health)}
        </div>
      </button>
      <span
        className={`badge${health?.reachable ? " badge-success" : health === undefined ? "" : " badge-danger"}`}
      >
        {status}
      </span>
      <button
        className="btn-icon"
        aria-label={`Recheck ${p.name}`}
        title="Recheck"
        onClick={onRecheck}
      >
        {health === undefined ? <Spinner size={13} /> : <ArrowClockwiseIcon size={15} />}
      </button>
      <button className="btn" aria-label={`Edit ${p.name}`} onClick={onEdit}>
        Edit
      </button>
      <button
        className={`btn${p.systemPrompt ? " text-accent" : ""}`}
        aria-label={`${p.name} system prompt`}
        aria-expanded={prompting}
        onClick={onPrompt}
      >
        System Prompt
      </button>
      <button
        className="toggle"
        role="switch"
        aria-label={`Use ${p.name}`}
        aria-checked={p.enabled}
        onClick={onToggle}
      />
    </div>
  );
}

/** Under a server that stopped answering: look for where it moved, or type its new address. */
function Repair({ provider: p, onMoved }: { provider: ApiProviderView; onMoved: () => void }) {
  const [busy, setBusy] = useState(false);
  const [address, setAddress] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const run = async (task: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await task();
      setAddress(null);
      onMoved();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="flex flex-col gap-1.5 border-b border-border py-2">
      {address === null ? (
        <div className="flex gap-2">
          <button
            className="btn"
            aria-label={`Find where ${p.name} moved`}
            disabled={busy}
            onClick={() => void run(() => api.relocateApiProvider(p.id))}
          >
            {busy ? <Spinner size={13} /> : <MagnifyingGlassIcon size={13} />}
            Find server
          </button>
          <button className="btn" disabled={busy} onClick={() => setAddress(p.baseUrl)}>
            Change address
          </button>
        </div>
      ) : (
        <form
          className="flex gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            void run(() => api.saveApiProvider({ ...p, baseUrl: address }, { action: "keep" }));
          }}
        >
          <input
            className="input min-w-0 flex-1 font-[family-name:var(--font-mono)] text-[length:var(--fs-sm)]"
            aria-label={`${p.name} address`}
            value={address}
            onChange={(e) => setAddress(e.target.value)}
            autoFocus
          />
          <button className="btn" type="submit" disabled={busy}>
            {busy && <Spinner size={13} />}
            Save
          </button>
          <button className="btn" type="button" onClick={() => setAddress(null)}>
            Cancel
          </button>
        </form>
      )}
      {error && (
        <div role="alert" className="text-[length:var(--fs-sm)] text-danger">
          {error}
        </div>
      )}
    </div>
  );
}

/** A local server's state in words, for its tile's name, and the colour of its dot. */
function tileStatus(p: ApiProviderView, health: ProviderHealth | undefined): [string, string] {
  if (!p.enabled) return ["off", "bg-fg-subtle"];
  if (health === undefined) return ["checking", "bg-border-strong"];
  return health.reachable ? ["answering", "bg-success"] : ["not answering", "bg-danger"];
}

/**
 * The Local providers section of Settings → Providers: a tile for each model server on this
 * computer, with a dot for whether it is answering. A saved server's tile opens its row, with its
 * status, switch, and system prompt; a server running here that is not added yet adds it.
 */
export default function LocalProviders({
  providers,
  onEdit,
  onChanged,
}: {
  /** The saved providers that run on this computer. */
  providers: ApiProviderView[];
  onEdit: (provider: ApiProvider) => void;
  /** Reloads the saved providers after one changed here. */
  onChanged: () => Promise<void>;
}) {
  const [health, setHealth] = useState<HealthMap>(() => Object.fromEntries(healthCache));
  const [promptingId, setPromptingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);

  // Re-probes without clearing the last result first, so a row keeps its status until the fresh
  // one lands rather than blinking through "Checking…".
  const probe = useCallback((id: string) => {
    const apply = (result: ProviderHealth) => {
      healthCache.set(id, result);
      setHealth((h) => ({ ...h, [id]: result }));
    };
    api.probeApiProvider(id).then(apply, (e) =>
      apply({
        reachable: false,
        latencyMs: null,
        servedModels: [],
        error: errorText(e),
        images: {},
      }),
    );
  }, []);

  // Drops cached health for servers that are gone, then probes the current ones.
  const ids = providers.map((p) => p.id).join("\n");
  useEffect(() => {
    const current = ids ? ids.split("\n") : [];
    for (const id of [...healthCache.keys()]) if (!current.includes(id)) healthCache.delete(id);
    current.forEach(probe);
  }, [ids, probe]);

  // Servers answering on this computer, looked for each time the pane opens.
  const [found, setFound] = useState<DiscoveredServer[] | null>(null);
  useEffect(() => {
    api.discoverInferenceServers().then(setFound, () => setFound([]));
  }, []);
  const saved = new Set(providers.map((p) => p.baseUrl.replace(/\/+$/, "")));
  const unsaved = (found ?? []).filter((d) => !saved.has(d.baseUrl.replace(/\/+$/, "")));
  const empty = found !== null && providers.length === 0 && unsaved.length === 0;

  const toggle = async (p: ApiProviderView) => {
    setError(null);
    try {
      await api.saveApiProvider({ ...p, enabled: !p.enabled }, { action: "keep" });
      await onChanged();
    } catch (e) {
      setError(errorText(e));
    }
  };

  const savePrompt = async (p: ApiProviderView, systemPrompt: string) => {
    await api.saveApiProvider({ ...p, systemPrompt }, { action: "keep" });
    await onChanged();
    setPromptingId(null);
  };

  const open = providers.find((p) => p.id === selected);

  return (
    <>
      {(providers.length > 0 || unsaved.length > 0) && (
        <div role="group" aria-label="Local providers" className={TILE_GRID}>
          {providers.map((p) => {
            const [status, dot] = tileStatus(p, health[p.id]);
            return (
              <button
                key={p.id}
                className={`${PROVIDER_TILE} bg-panel text-fg ${selected === p.id ? "border-accent" : "border-border"}`}
                aria-label={`${p.name}, ${status}`}
                aria-expanded={selected === p.id}
                title={healthLine(p, health[p.id])}
                onClick={() => setSelected(selected === p.id ? null : p.id)}
              >
                <span
                  aria-hidden="true"
                  className={`absolute top-2 right-2 size-2 rounded-full ${dot}`}
                />
                <ProviderLogos kind="api" preset={p.preset} logo={p.logo} size={22} />
                <span className="max-w-full truncate">{p.name}</span>
              </button>
            );
          })}
          {unsaved.map((d) => (
            <button
              key={d.baseUrl}
              className={`${PROVIDER_TILE} border-border bg-panel text-fg`}
              aria-label={`Add ${d.name}`}
              title={d.baseUrl}
              onClick={() => onEdit(fromDiscovered(d))}
            >
              <ProviderLogos kind="api" preset={presetOf(d)} size={22} />
              <span className="max-w-full truncate">{d.name}</span>
            </button>
          ))}
        </div>
      )}
      {open && (
        <>
          <ServerRow
            provider={open}
            health={health[open.id]}
            prompting={promptingId === open.id}
            onRecheck={() => probe(open.id)}
            onEdit={() => onEdit(open)}
            onPrompt={() => setPromptingId(promptingId === open.id ? null : open.id)}
            onToggle={() => void toggle(open)}
          />
          {health[open.id]?.reachable === false && (
            <Repair provider={open} onMoved={() => void onChanged().then(() => probe(open.id))} />
          )}
          {promptingId === open.id && (
            <PromptEditor
              label={open.name}
              saved={open.systemPrompt}
              onSave={(text) => savePrompt(open, text)}
              onCancel={() => setPromptingId(null)}
            />
          )}
        </>
      )}
      {empty && <div className="py-6 text-center text-fg-muted">No local providers available</div>}
      {error && (
        <div role="alert" className="text-[length:var(--fs-sm)] text-danger">
          {error}
        </div>
      )}
    </>
  );
}
