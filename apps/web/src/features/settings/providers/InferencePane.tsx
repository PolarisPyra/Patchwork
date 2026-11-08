import { Fragment, useCallback, useEffect, useState } from "react";
import { ArrowClockwiseIcon, MagnifyingGlassIcon, PlusIcon } from "@phosphor-icons/react";
import {
  api,
  type ApiProvider,
  type ApiProviderView,
  type DiscoveredServer,
  type ProviderHealth,
} from "~/api";
import ProviderLogos from "~/components/ProviderLogos";
import Spinner from "~/components/Spinner";
import { blank, ProviderForm, syncAgents } from "~/features/settings/providers/ApiProvidersPane";
import { PromptEditor } from "~/features/settings/providers/ProvidersPane";
import type { PaneProps } from "~/features/settings/types";
import { errorText } from "~/lib/util";

/** The probe of each server, by provider id: `undefined` while it is still checking. */
type HealthMap = Partial<Record<string, ProviderHealth>>;

// Kept across mounts so reopening the Inference tab shows the last servers and their status at
// once, then refreshes in place, instead of flashing every row back to "Checking…".
let providerCache: ApiProviderView[] = [];
const healthCache = new Map<string, ProviderHealth>();

/** A custom provider prefilled from a server found on this computer, so Add opens the editor ready
 * to save. */
function fromDiscovered(d: DiscoveredServer): ApiProvider {
  return {
    ...blank({ id: "custom", name: "", baseUrl: "", models: [] }),
    local: true,
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

/** A server answering on this computer that is not saved yet, offered to add. */
function FoundRow({ server, onAdd }: { server: DiscoveredServer; onAdd: () => void }) {
  return (
    <div className="my-1.5 flex items-center gap-3 rounded-[var(--radius-sm)] border border-border px-2.5 py-2">
      <div className="min-w-0 flex-1">
        <div className="font-semibold">{server.name}</div>
        <div className="truncate font-mono text-[length:var(--fs-sm)] text-fg-muted">
          {server.baseUrl} · {server.models.length} model{server.models.length === 1 ? "" : "s"}
        </div>
      </div>
      <button className="btn" aria-label={`Add ${server.name}`} onClick={onAdd}>
        <PlusIcon size={13} />
        Add
      </button>
    </div>
  );
}

/**
 * Settings → Local providers: the local-first home for the OpenAI-compatible model servers Patchwork
 * talks to. Each server shows whether it is answering right now and which models it serves, so a
 * model's availability does not depend on what a CLI happens to have configured on its host.
 */
export default function InferencePane({ updateSettings }: PaneProps) {
  const [providers, setProviders] = useState<ApiProviderView[]>(providerCache);
  const [health, setHealth] = useState<HealthMap>(() => Object.fromEntries(healthCache));
  const [editing, setEditing] = useState<ApiProvider | null>(null);
  const [promptingId, setPromptingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

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

  const reload = useCallback(async () => {
    try {
      const list = await api.listApiProviders();
      // Inference manages local servers only; cloud endpoints live in Settings → Cloud providers.
      const local = list.filter((p) => p.local);
      providerCache = local;
      setProviders(local);
      // `api` is usable when any provider has a model, so sync on the full list, not just local.
      syncAgents(list, updateSettings);
      // Drop cached health for servers that are gone, then re-probe the current ones.
      const ids = new Set(local.map((p) => p.id));
      for (const id of [...healthCache.keys()]) if (!ids.has(id)) healthCache.delete(id);
      local.forEach((p) => probe(p.id));
    } catch (e) {
      setError(errorText(e));
    }
  }, [updateSettings, probe]);

  useEffect(() => {
    void reload();
  }, [reload]);

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
      await reload();
    } catch (e) {
      setError(errorText(e));
    }
  };

  const savePrompt = async (p: ApiProviderView, systemPrompt: string) => {
    await api.saveApiProvider({ ...p, systemPrompt }, { action: "keep" });
    await reload();
    setPromptingId(null);
  };

  if (editing !== null) {
    return (
      <ProviderForm
        key={editing.id}
        initial={editing}
        hasKey={providers.some((p) => p.id === editing.id && p.hasKey)}
        onDone={() => {
          setEditing(null);
          void reload();
        }}
      />
    );
  }

  return (
    <>
      {providers.map((p) => (
        <Fragment key={p.id}>
          <ServerRow
            provider={p}
            health={health[p.id]}
            prompting={promptingId === p.id}
            onRecheck={() => probe(p.id)}
            onEdit={() => setEditing(p)}
            onPrompt={() => setPromptingId(promptingId === p.id ? null : p.id)}
            onToggle={() => void toggle(p)}
          />
          {health[p.id]?.reachable === false && (
            <Repair provider={p} onMoved={() => void reload()} />
          )}
          {promptingId === p.id && (
            <PromptEditor
              label={p.name}
              saved={p.systemPrompt}
              onSave={(text) => savePrompt(p, text)}
              onCancel={() => setPromptingId(null)}
            />
          )}
        </Fragment>
      ))}
      {unsaved.map((d) => (
        <FoundRow key={d.baseUrl} server={d} onAdd={() => setEditing(fromDiscovered(d))} />
      ))}
      {empty && (
        <div className="flex h-full items-center justify-center text-fg-muted">
          No local providers available
        </div>
      )}
      {error && (
        <div role="alert" className="text-[length:var(--fs-sm)] text-danger">
          {error}
        </div>
      )}
    </>
  );
}
