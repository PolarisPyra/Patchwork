import { Fragment, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  api,
  AGENT_LABELS,
  type AgentKind,
  type AgentStatus,
  type ApiProvider,
  type ApiProviderView,
  type CliKind,
} from "~/api";
import ProviderLogos from "~/components/ProviderLogos";
import {
  ApiProviderRow,
  blank,
  PresetGrid,
  ProviderForm,
  syncAgents,
} from "~/features/settings/components/ApiProvidersPane";
import { TEXTAREA, TokenRow } from "~/features/settings/components/controls";
import { DefaultModel } from "~/features/settings/components/TokensPane";
import type { PaneProps } from "~/features/settings/types";
import { AGENT_KINDS, CLI_KINDS, describeAgent, errorText } from "~/lib/util";

/** Downloads and runs an agent CLI's official installer, then re-checks it. */
function CliInstall({ kind, onInstalled }: { kind: CliKind; onInstalled: () => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const install = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.installCli(kind);
      onInstalled();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <>
      <button className="btn" onClick={install} disabled={busy}>
        {busy ? "Installing…" : "Download"}
      </button>
      {error && <span className="text-[length:var(--fs-sm)] text-danger">{error}</span>}
    </>
  );
}

/** One row per CLI: whether it is installed, where it is, and whether Patchwork connects to it. */

/** A provider's system prompt, with Save and Cancel under the field. Save waits for `onSave`
 * and shows its error here; the parent closes the editor once it has saved. */
function PromptEditor({
  label,
  saved,
  onSave,
  onCancel,
}: {
  label: string;
  saved: string;
  onSave: (text: string) => Promise<void>;
  onCancel: () => void;
}) {
  const [draft, setDraft] = useState(saved);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      await onSave(draft);
    } catch (e) {
      setError(errorText(e));
      setBusy(false);
    }
  };
  return (
    <div className="flex flex-col gap-2 border-b border-border py-2.5">
      <textarea
        className={TEXTAREA}
        aria-label={`${label} system prompt`}
        value={draft}
        autoFocus
        spellCheck
        onChange={(e) => setDraft(e.target.value)}
      />
      {error && <div className="text-[length:var(--fs-sm)] text-danger">{error}</div>}
      <div className="flex items-center gap-2">
        <button className="btn" disabled={busy || !draft} onClick={() => setDraft("")}>
          Clear
        </button>
        <button className="btn" disabled={busy} onClick={onCancel}>
          Cancel
        </button>
        <button
          className="btn btn-primary"
          disabled={busy || draft === saved}
          onClick={() => void save()}
        >
          {busy ? "Saving…" : "Save"}
        </button>
      </div>
    </div>
  );
}

/** A CLI's system prompt, saved in the settings. */
function SystemPrompt({
  kind,
  settings,
  updateSettings,
  onDone,
}: PaneProps & { kind: AgentKind; onDone: () => void }) {
  return (
    <PromptEditor
      label={AGENT_LABELS[kind]}
      saved={settings.systemPrompts[kind] ?? ""}
      onSave={async (text) => {
        await updateSettings((st) => {
          const { [kind]: _, ...rest } = st.systemPrompts;
          return { ...st, systemPrompts: text.trim() ? { ...rest, [kind]: text } : rest };
        });
        onDone();
      }}
      onCancel={onDone}
    />
  );
}

function CliProviderRows({ settings, updateSettings }: PaneProps) {
  const [prompting, setPrompting] = useState<AgentKind | null>(null);
  const [status, setStatus] = useState<Partial<Record<AgentKind, AgentStatus | null>>>({});
  // Re-reads an agent, skipping the cache, so a fresh install shows at once.
  const recheck = (kind: AgentKind) =>
    api.detectAgent(kind, true).then(
      (s) => setStatus((prev) => ({ ...prev, [kind]: s })),
      () => setStatus((prev) => ({ ...prev, [kind]: null })),
    );
  const refresh = () => CLI_KINDS.forEach(recheck);
  const setPath = async (kind: AgentKind, path: string | undefined) => {
    setStatus((prev) => ({ ...prev, [kind]: undefined }));
    await updateSettings((st) => {
      const { [kind]: _, ...rest } = st.agentPaths;
      return { ...st, agentPaths: path ? { ...rest, [kind]: path } : rest };
    });
    await recheck(kind);
  };
  const locate = async (kind: AgentKind) => {
    const path = await open({ multiple: false, directory: false });
    if (typeof path === "string") await setPath(kind, path);
  };

  // Shows the last reading at once, then reads each agent again, and again whenever the window
  // comes back, so a CLI installed or removed meanwhile shows without restarting.
  useEffect(() => {
    let cancelled = false;
    const detect = (force: boolean) => {
      for (const kind of CLI_KINDS) {
        api.detectAgent(kind, force).then(
          (s) => {
            if (!cancelled) setStatus((prev) => ({ ...prev, [kind]: s }));
          },
          () => {
            if (!cancelled) setStatus((prev) => ({ ...prev, [kind]: null }));
          },
        );
      }
    };
    detect(false);
    detect(true);
    const onFocus = () => detect(true);
    window.addEventListener("focus", onFocus);
    return () => {
      cancelled = true;
      window.removeEventListener("focus", onFocus);
    };
  }, []);

  const connected = settings.agents;
  const toggle = (kind: AgentKind) =>
    void updateSettings((st) => ({
      ...st,
      agents: st.agents.includes(kind) ? st.agents.filter((k) => k !== kind) : [...st.agents, kind],
    }));
  return (
    <>
      {CLI_KINDS.map((kind) => {
        const s = status[kind];
        const on = connected.includes(kind);
        const path = settings.agentPaths[kind];
        const badge = s === undefined ? "Checking…" : s?.installed ? "Installed" : "Not installed";
        // Keep at least one agent connected, and only connect installed ones.
        const locked = on ? connected.length === 1 : !s?.installed;
        return (
          <Fragment key={kind}>
            <div className="flex items-center gap-3 border-b border-border py-2.5">
              <ProviderLogos kind={kind} size={20} />
              <div className="min-w-0 flex-1">
                <div className="font-semibold">{AGENT_LABELS[kind]}</div>
                <div className="text-[length:var(--fs-sm)] text-fg-muted">{describeAgent(s)}</div>
              </div>
              <span className={`badge${s?.installed ? " badge-success" : ""}`}>{badge}</span>
              {!s?.installed && s !== undefined && !path && (
                <CliInstall kind={kind} onInstalled={refresh} />
              )}
              {path ? (
                <button className="btn" title={path} onClick={() => void setPath(kind, undefined)}>
                  Auto-detect
                </button>
              ) : (
                s !== undefined &&
                !s?.installed && (
                  <button className="btn" onClick={() => void locate(kind)}>
                    Locate…
                  </button>
                )
              )}
              <button
                className={`btn${settings.systemPrompts[kind] ? " text-accent" : ""}`}
                aria-expanded={prompting === kind}
                onClick={() => setPrompting(prompting === kind ? null : kind)}
              >
                System Prompt
              </button>
              <button
                className="toggle"
                role="switch"
                aria-label={`Connect ${AGENT_LABELS[kind]}`}
                aria-checked={on}
                disabled={locked}
                onClick={() => toggle(kind)}
              />
            </div>
            {prompting === kind && (
              <SystemPrompt
                kind={kind}
                settings={settings}
                updateSettings={updateSettings}
                onDone={() => setPrompting(null)}
              />
            )}
          </Fragment>
        );
      })}
    </>
  );
}

/** Every provider in one list: the CLIs, then the API endpoints. Editing an endpoint takes over
 * the pane with its form, as it did on its own page. */
export default function ProvidersPane({ settings, updateSettings }: PaneProps) {
  const [providers, setProviders] = useState<ApiProviderView[]>([]);
  const [editing, setEditing] = useState<ApiProvider | null>(null);
  const [promptingId, setPromptingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = async () => {
    try {
      const list = await api.listApiProviders();
      setProviders(list);
      syncAgents(list, updateSettings);
    } catch (e) {
      setError(errorText(e));
    }
  };

  useEffect(() => {
    void reload();
  }, []);

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

  // Choosing a default reorders `agents`, not the buttons.
  const connected = settings.agents;
  const makeDefault = (kind: AgentKind) =>
    void updateSettings((st) => ({
      ...st,
      agents: [kind, ...st.agents.filter((k) => k !== kind)],
    }));

  return (
    <>
      <CliProviderRows settings={settings} updateSettings={updateSettings} />
      {providers.map((p) => (
        <Fragment key={p.id}>
          <ApiProviderRow
            provider={p}
            prompting={promptingId === p.id}
            onEdit={() => setEditing(p)}
            onPrompt={() => setPromptingId(promptingId === p.id ? null : p.id)}
            onToggle={() => void toggle(p)}
          />
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
      {connected.includes("api") && (
        <TokenRow title="Vision model">
          <DefaultModel
            kind="api"
            label="Vision model"
            empty="Default"
            value={settings.visionModel ?? undefined}
            onChange={(id) => void updateSettings((s) => ({ ...s, visionModel: id ?? null }))}
          />
        </TokenRow>
      )}
      {connected.length > 1 && (
        <TokenRow title="Default for new tasks">
          <select
            aria-label="Default for new tasks"
            className="h-7 w-[200px] rounded-[var(--radius-sm)] border border-border-strong bg-editor pl-2 pr-6 text-[length:var(--fs-sm)] text-fg"
            value={connected[0]}
            onChange={(e) => makeDefault(e.target.value as AgentKind)}
          >
            {AGENT_KINDS.filter((kind) => connected.includes(kind)).map((kind) => (
              <option key={kind} value={kind}>
                {AGENT_LABELS[kind]}
              </option>
            ))}
          </select>
        </TokenRow>
      )}
      <PresetGrid onPick={(preset) => setEditing(blank(preset))} />
      {error && <div className="text-[length:var(--fs-sm)] text-danger">{error}</div>}
    </>
  );
}
