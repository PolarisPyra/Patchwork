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
  type ModelOption,
} from "~/api";
import ProviderLogos from "~/components/ProviderLogos";
import CliPane, { hasDetails, type DetailsKind } from "~/features/settings/providers/CliPane";
import {
  ApiProviderRow,
  blank,
  PresetGrid,
  ProviderForm,
  syncAgents,
} from "~/features/settings/providers/ApiProvidersPane";
import { TEXTAREA, TokenRow } from "~/features/settings/controls";
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
      {error && (
        <span role="alert" className="text-[length:var(--fs-sm)] text-danger">
          {error}
        </span>
      )}
    </>
  );
}

/** A provider's system prompt, with Save and Cancel under the field. Save waits for `onSave`
 * and shows its error here; the parent closes the editor once it has saved. Shared with the
 * Inference pane, which edits each inference server's prompt the same way. */
export function PromptEditor({
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
      {error && (
        <div role="alert" className="text-[length:var(--fs-sm)] text-danger">
          {error}
        </div>
      )}
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

/** One row per CLI: whether it is installed, where it is, and whether Patchwork connects to it. */
function CliProviderRows({
  settings,
  updateSettings,
  onDetails,
}: PaneProps & { onDetails: (kind: DetailsKind) => void }) {
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
    try {
      await updateSettings((st) => {
        const { [kind]: _, ...rest } = st.agentPaths;
        return { ...st, agentPaths: path ? { ...rest, [kind]: path } : rest };
      });
    } finally {
      // Read again even if the path was not saved, so the row leaves "Checking…".
      await recheck(kind);
    }
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
        // Only installed agents can be switched on; any agent can be switched off.
        const locked = !on && !s?.installed;
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
                <button
                  className="btn"
                  title={path}
                  aria-label={`Auto-detect ${AGENT_LABELS[kind]} path`}
                  onClick={() => void setPath(kind, undefined)}
                >
                  Auto-detect
                </button>
              ) : (
                s !== undefined &&
                !s?.installed && (
                  <button
                    className="btn"
                    aria-label={`Locate ${AGENT_LABELS[kind]}`}
                    onClick={() => void locate(kind)}
                  >
                    Locate…
                  </button>
                )
              )}
              {s?.installed && hasDetails(kind) && (
                <button
                  className="btn"
                  aria-label={`${AGENT_LABELS[kind]} details`}
                  onClick={() => onDetails(kind)}
                >
                  Details
                </button>
              )}
              <button
                className={`btn${settings.systemPrompts[kind] ? " text-accent" : ""}`}
                aria-label={`${AGENT_LABELS[kind]} system prompt`}
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

/** Agents whose models can read a message's images for an API model. */
const VISION_AGENTS: AgentKind[] = ["api", "claude", "codex"];

/** The model that reads a message's images and describes them to an API chat's model, picked from
 * every connected agent that can read them. The empty choice sends the images to the chat's model. */
function VisionModelPicker({ settings, updateSettings }: PaneProps) {
  const kinds = AGENT_KINDS.filter((k) => VISION_AGENTS.includes(k) && settings.agents.includes(k));
  const kindsKey = kinds.join(",");
  const [models, setModels] = useState<Partial<Record<AgentKind, ModelOption[]>>>({});
  useEffect(() => {
    let cancelled = false;
    for (const kind of kindsKey.split(",") as AgentKind[]) {
      api.listModels(kind).then(
        (list) => {
          if (!cancelled) setModels((m) => ({ ...m, [kind]: list }));
        },
        () => {},
      );
    }
    return () => {
      cancelled = true;
    };
  }, [kindsKey]);
  const vision = settings.vision;
  const pick = (value: string) => {
    const at = value.indexOf(":");
    const next =
      at < 0 ? null : { agent: value.slice(0, at) as AgentKind, model: value.slice(at + 1) };
    void updateSettings((s) => ({ ...s, vision: next }));
  };
  return (
    <select
      aria-label="Vision model"
      className="h-7 w-[200px] rounded-[var(--radius-sm)] border border-border-strong bg-editor pl-2 pr-6 text-[length:var(--fs-sm)] text-fg"
      value={vision ? `${vision.agent}:${vision.model}` : ""}
      onChange={(e) => pick(e.target.value)}
    >
      <option value="">The chat's own model</option>
      {kinds.map((kind) => {
        const listed = (models[kind] ?? []).flatMap((m) =>
          m.id ? [{ id: m.id, label: m.label }] : [],
        );
        // A saved model that is no longer listed still shows as picked.
        if (vision?.agent === kind && !listed.some((m) => m.id === vision.model)) {
          listed.push({ id: vision.model, label: vision.model });
        }
        return (
          <optgroup key={kind} label={AGENT_LABELS[kind]}>
            {listed.map((m) => (
              <option key={m.id} value={`${kind}:${m.id}`}>
                {m.label}
              </option>
            ))}
          </optgroup>
        );
      })}
    </select>
  );
}

export default function ProvidersPane({ settings, updateSettings }: PaneProps) {
  // Only cloud endpoints live here; local model servers are managed in Settings → Local providers.
  const [providers, setProviders] = useState<ApiProviderView[]>([]);
  const [editing, setEditing] = useState<ApiProvider | null>(null);
  const [promptingId, setPromptingId] = useState<string | null>(null);
  const [details, setDetails] = useState<DetailsKind | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = async () => {
    try {
      const list = await api.listApiProviders();
      setProviders(list);
      // `api` is usable when any provider has a model, local or cloud, so sync on the full list.
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

  if (details !== null) return <CliPane kind={details} onBack={() => setDetails(null)} />;

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

  const cloud = providers.filter((p) => !p.local);
  // Choosing a default reorders `agents`, not the buttons.
  const connected = settings.agents;
  const makeDefault = (kind: AgentKind) =>
    void updateSettings((st) => ({
      ...st,
      agents: [kind, ...st.agents.filter((k) => k !== kind)],
    }));

  return (
    <>
      <CliProviderRows settings={settings} updateSettings={updateSettings} onDetails={setDetails} />
      {cloud.map((p) => (
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
          <VisionModelPicker settings={settings} updateSettings={updateSettings} />
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
      {error && (
        <div role="alert" className="text-[length:var(--fs-sm)] text-danger">
          {error}
        </div>
      )}
    </>
  );
}
