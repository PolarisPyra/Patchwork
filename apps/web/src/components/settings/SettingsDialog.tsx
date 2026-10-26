import { useCallback, useEffect, useState, type ReactNode } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { RELEASES_URL, restartToUpdate } from "~/lib/updates";
import {
  ArrowCircleUpIcon,
  BellIcon,
  BooksIcon,
  CaretDownIcon,
  DownloadSimpleIcon,
  FolderSimpleIcon,
  GaugeIcon,
  KeyboardIcon,
  MinusIcon,
  PaletteIcon,
  PlugsConnectedIcon,
  PlusIcon,
  RobotIcon,
  TerminalIcon,
  TrashIcon,
  XIcon,
  type Icon,
} from "@phosphor-icons/react";
import {
  api,
  onUpdateStatus,
  AGENT_LABELS,
  importSourcesFor,
  SOURCE_LABELS,
  type AgentKind,
  type AgentStatus,
  type ImportScan,
  type ImportSource,
  type ModelOption,
  type NotificationSettings,
  type Settings,
  type ShellChoices,
  type SkillsView,
  type Theme,
  type TokenSettings,
  type UpdateState,
  type UpdateStatus,
} from "~/api";
import ProviderLogos from "~/components/ProviderLogos";
import type { UpdateSettings } from "~/app/App";
import { syncProjects } from "~/lib/projects";
import { formatTokens } from "~/lib/tokens";
import { AGENT_KINDS, CLI_KINDS, describeAgent, errorText } from "~/lib/util";
import logoManifest from "../../../../../assets/logos/manifest.json";
import { CHIP_CLASS } from "~/components/chat/ModelPicker";
import Dropdown from "~/components/workspace/Dropdown";
import KeybindingsPane from "~/components/settings/KeybindingsPane";
import ApiProvidersPane from "~/components/settings/ApiProvidersPane";

type Section =
  | "appearance"
  | "agent"
  | "api"
  | "tokens"
  | "notifications"
  | "imports"
  | "environment"
  | "skills"
  | "projects"
  | "shortcuts"
  | "updates";

const SECTIONS: { id: Section; label: string; Icon: Icon }[] = [
  { id: "appearance", label: "Appearance", Icon: PaletteIcon },
  { id: "agent", label: "CLI Providers", Icon: RobotIcon },
  { id: "api", label: "API Providers", Icon: PlugsConnectedIcon },
  { id: "tokens", label: "AI Tokenomics", Icon: GaugeIcon },
  { id: "notifications", label: "Notifications", Icon: BellIcon },
  { id: "imports", label: "Imports", Icon: DownloadSimpleIcon },
  { id: "environment", label: "Environment", Icon: TerminalIcon },
  { id: "skills", label: "Skills", Icon: BooksIcon },
  { id: "projects", label: "Projects", Icon: FolderSimpleIcon },
  { id: "shortcuts", label: "Shortcuts", Icon: KeyboardIcon },
  { id: "updates", label: "Updates", Icon: ArrowCircleUpIcon },
];

interface PaneProps {
  settings: Settings;
  updateSettings: UpdateSettings;
}

interface SettingsDialogProps extends PaneProps {
  /** Called after conversations were imported, so threads can be reloaded. */
  onImported: () => void;
  onClose: () => void;
}

export default function SettingsDialog({
  settings,
  updateSettings,
  onImported,
  onClose,
}: SettingsDialogProps) {
  const [section, setSection] = useState<Section>("appearance");
  const title = SECTIONS.find((s) => s.id === section)?.label ?? "";
  const paneProps = { settings, updateSettings };

  return (
    <div
      className="fixed inset-0 z-[100] flex items-center justify-center bg-[rgba(0,0,0,0.4)]"
      onMouseDown={onClose}
    >
      <div
        className="flex h-[680px] max-h-[calc(100vh_-_48px)] w-[920px] max-w-[calc(100vw_-_48px)] overflow-hidden rounded-[var(--radius-lg)] border border-border-strong bg-elevated shadow-[var(--shadow-popup)]"
        role="dialog"
        aria-modal="true"
        aria-label="Settings"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <nav className="flex w-[180px] flex-none flex-col gap-0.5 border-r border-border bg-panel px-2 py-2.5">
          {SECTIONS.map(({ id, label, Icon }) => (
            <button
              key={id}
              className={`flex h-8 cursor-pointer items-center gap-2.5 rounded-[var(--radius-sm)] border-none px-2 text-left text-fg ${section === id ? "bg-selected hover:bg-[color-mix(in_srgb,var(--bg-selected),var(--text)_8%)]" : "bg-transparent hover:bg-hover"}`}
              onClick={() => setSection(id)}
            >
              <Icon size={18} className="flex-none" />
              {label}
            </button>
          ))}
        </nav>
        <div className="flex min-w-0 flex-1 flex-col">
          <div className="flex h-12 flex-none items-center justify-between pr-4 pl-5">
            <h2 className="m-0 text-[length:var(--fs-lg)] font-semibold">{title}</h2>
            <button className="btn-icon" title="Close" aria-label="Close" onClick={onClose}>
              <XIcon size={16} />
            </button>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto px-5 pb-5">
            {section === "appearance" && <AppearancePane {...paneProps} />}
            {section === "agent" && <AgentPane {...paneProps} />}
            {section === "api" && <ApiProvidersPane updateSettings={updateSettings} />}
            {section === "tokens" && <TokensPane {...paneProps} />}
            {section === "notifications" && <NotificationsPane {...paneProps} />}
            {section === "imports" && <ImportsPane {...paneProps} onImported={onImported} />}
            {section === "environment" && <EnvironmentPane {...paneProps} />}
            {section === "skills" && <SkillsPane {...paneProps} />}
            {section === "projects" && <ProjectsPane {...paneProps} />}
            {section === "shortcuts" && <KeybindingsPane />}
            {section === "updates" && <UpdatesPane {...paneProps} />}
          </div>
        </div>
      </div>
    </div>
  );
}

const THEMES: { value: Theme; label: string }[] = [
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
  { value: "system", label: "System" },
];

/** Preview image of each app icon color way, keyed by id. */
const LOGO_URLS: Record<string, string> = Object.fromEntries(
  Object.entries(
    import.meta.glob<string>("../../../../../assets/logos/*.svg", {
      eager: true,
      query: "?url",
      import: "default",
    }),
  ).map(([path, url]) => [path.slice(path.lastIndexOf("/") + 1, -".svg".length), url]),
);

function AppearancePane({ settings, updateSettings }: PaneProps) {
  const logo = settings.logo ?? logoManifest.default;
  return (
    <>
      <ThemeRow settings={settings} updateSettings={updateSettings} />
      <div className="border-b border-border py-2.5">
        <div className="font-semibold">App icon</div>
        <div role="radiogroup" aria-label="App icon" className="mt-2 grid grid-cols-8 gap-1">
          {logoManifest.icons.map((icon) => (
            <button
              key={icon.id}
              role="radio"
              aria-checked={logo === icon.id}
              aria-label={icon.name}
              title={icon.name}
              className={`grid aspect-square cursor-pointer place-items-center rounded-[var(--radius-sm)] border p-2 ${logo === icon.id ? "border-accent bg-accent-soft" : "border-transparent bg-transparent hover:bg-hover"}`}
              onClick={() => void updateSettings((s) => ({ ...s, logo: icon.id }))}
            >
              <img src={LOGO_URLS[icon.id]} alt="" draggable={false} className="size-full" />
            </button>
          ))}
        </div>
      </div>
    </>
  );
}

function ThemeRow({ settings, updateSettings }: PaneProps) {
  return (
    <div className="flex items-center gap-3 border-b border-border py-2.5">
      <div className="min-w-0 flex-1">
        <div className="font-semibold">Theme</div>
        <div className="text-[length:var(--fs-sm)] text-fg-muted">
          Light, dark, or follow the system setting
        </div>
      </div>
      <div className="inline-flex overflow-hidden rounded-[var(--radius-sm)] border border-border-strong">
        {THEMES.map((t) => (
          <button
            key={t.value}
            className={`grid h-6 w-auto cursor-pointer place-items-center border-none px-2.5 py-0 text-[length:var(--fs-sm)] [&+&]:border-l [&+&]:border-border-strong ${settings.theme === t.value ? "bg-selected text-fg" : "bg-transparent text-fg-muted hover:bg-hover hover:text-fg"}`}
            onClick={() => void updateSettings((s) => ({ ...s, theme: t.value }))}
          >
            {t.label}
          </button>
        ))}
      </div>
    </div>
  );
}

/** Context sizes offered by the token settings. */
const TOKEN_LEVELS = [100_000, 150_000, 200_000, 300_000];

/** Context sizes to compact at; 0 is Default. */
const COMPACT_WINDOWS = [0, 200_000, 300_000, 500_000];

/** One setting: its name and what it does, with the control on the right. */
function TokenRow({
  title,
  desc,
  children,
}: {
  title: string;
  desc?: string;
  children: ReactNode;
}) {
  return (
    <div className="flex items-center gap-3 border-b border-border py-2.5">
      <div className="min-w-0 flex-1">
        <div className="font-semibold">{title}</div>
        {desc && <div className="text-[length:var(--fs-sm)] text-fg-muted">{desc}</div>}
      </div>
      {children}
    </div>
  );
}

function TokenSwitch({
  label,
  on,
  onChange,
}: {
  label: string;
  on: boolean;
  onChange: (on: boolean) => void;
}) {
  return (
    <button
      className="toggle"
      role="switch"
      aria-label={label}
      aria-checked={on}
      onClick={() => onChange(!on)}
    />
  );
}

function ContextLevels({
  label,
  value,
  onChange,
  levels = TOKEN_LEVELS,
}: {
  label: string;
  value: number;
  onChange: (value: number) => void;
  levels?: number[];
}) {
  return (
    <div
      role="radiogroup"
      aria-label={label}
      className="inline-flex overflow-hidden rounded-[var(--radius-sm)] border border-border-strong"
    >
      {levels.map((level) => (
        <button
          key={level}
          role="radio"
          aria-checked={value === level}
          className={`grid h-6 w-auto cursor-pointer place-items-center border-none px-2.5 py-0 text-[length:var(--fs-sm)] tabular-nums [&+&]:border-l [&+&]:border-border-strong ${value === level ? "bg-selected text-fg" : "bg-transparent text-fg-muted hover:bg-hover hover:text-fg"}`}
          onClick={() => onChange(level)}
        >
          {level === 0 ? "Default" : formatTokens(level)}
        </button>
      ))}
    </div>
  );
}

/** The model new tasks of one agent start with. The CLI's own default is the empty choice. */
function DefaultModel({
  kind,
  value,
  onChange,
}: {
  kind: AgentKind;
  value: string | undefined;
  onChange: (id: string | undefined) => void;
}) {
  const [models, setModels] = useState<ModelOption[]>([]);
  useEffect(() => {
    let cancelled = false;
    api.listModels(kind).then(
      (list) => {
        if (!cancelled) setModels(list);
      },
      () => {
        if (!cancelled) setModels([]);
      },
    );
    return () => {
      cancelled = true;
    };
  }, [kind]);
  return (
    <select
      aria-label={`${AGENT_LABELS[kind]} model for new tasks`}
      className="h-7 w-[200px] rounded-[var(--radius-sm)] border border-border-strong bg-editor px-2 text-[length:var(--fs-sm)] text-fg"
      value={value ?? ""}
      onChange={(e) => onChange(e.target.value || undefined)}
    >
      <option value="">{kind === "api" ? "First model" : "CLI default"}</option>
      {models.flatMap((m) =>
        m.id
          ? [
              <option key={m.id} value={m.id}>
                {m.label}
              </option>,
            ]
          : [],
      )}
    </select>
  );
}

/** Ways to spend fewer tokens: what is shown, when to flag or compact a long context, and which
 * model new tasks start with. */
function TokensPane({ settings, updateSettings }: PaneProps) {
  const tokens = settings.tokens;
  const update = (patch: Partial<TokenSettings>) =>
    void updateSettings((s) => ({ ...s, tokens: { ...s.tokens, ...patch } }));
  const setDefaultModel = (kind: AgentKind, id: string | undefined) => {
    const next = { ...tokens.defaultModels };
    if (id) next[kind] = id;
    else delete next[kind];
    update({ defaultModels: next });
  };
  const connected = AGENT_KINDS.filter((k) => settings.agents.includes(k));
  return (
    <>
      <TokenRow
        title="Show token counts"
        desc="Context, cache reads and writes, input and output under each finished reply"
      >
        <TokenSwitch
          label="Show token counts"
          on={tokens.showUsage}
          onChange={(showUsage) => update({ showUsage })}
        />
      </TokenRow>
      <TokenRow
        title="Warn above"
        desc="Shows an above-threshold context warning below Worked for and above the divider"
      >
        <ContextLevels
          label="Warn above"
          value={tokens.warnAbove}
          onChange={(warnAbove) => update({ warnAbove })}
        />
      </TokenRow>
      <TokenRow
        title="Ask before resuming a large task"
        desc="When its prompt cache has expired, so resuming would re-read the whole context"
      >
        <TokenSwitch
          label="Ask before resuming a large task"
          on={tokens.askBeforeResume}
          onChange={(askBeforeResume) => update({ askBeforeResume })}
        />
      </TokenRow>
      <TokenRow
        title="Compact idle tasks"
        desc="Claude tasks are compacted about 55 minutes after their last reply, before the cache expires"
      >
        <TokenSwitch
          label="Compact idle tasks"
          on={tokens.autoCompact}
          onChange={(autoCompact) => update({ autoCompact })}
        />
      </TokenRow>
      <TokenRow
        title="Compact above"
        desc="Only idle tasks whose context is larger than this are compacted"
      >
        <ContextLevels
          label="Compact above"
          value={tokens.autoCompactAbove}
          levels={COMPACT_WINDOWS}
          onChange={(autoCompactAbove) => update({ autoCompactAbove })}
        />
      </TokenRow>
      <TokenRow
        title="Claude compacts at"
        desc="Compacts a working Claude task once its context reaches this. Default waits until the model's window is almost full"
      >
        <ContextLevels
          label="Claude compacts at"
          value={tokens.claudeCompactWindow}
          levels={COMPACT_WINDOWS}
          onChange={(claudeCompactWindow) => update({ claudeCompactWindow })}
        />
      </TokenRow>
      {connected.map((kind) => (
        <TokenRow
          key={kind}
          title={`${AGENT_LABELS[kind]} model`}
          desc="Used by new tasks. A smaller model spends fewer tokens on each turn"
        >
          <DefaultModel
            kind={kind}
            value={tokens.defaultModels[kind]}
            onChange={(id) => setDefaultModel(kind, id)}
          />
        </TokenRow>
      ))}
    </>
  );
}

/** Which finished work raises a desktop notification while Patchwork is not focused. */
function NotificationsPane({ settings, updateSettings }: PaneProps) {
  const notifications = settings.notifications;
  const update = (patch: Partial<NotificationSettings>) =>
    void updateSettings((s) => ({ ...s, notifications: { ...s.notifications, ...patch } }));
  return (
    <>
      <TokenRow
        title="Goal completed"
        desc="When a task meets its goal, while Patchwork is not the focused window"
      >
        <TokenSwitch
          label="Goal completed"
          on={notifications.goalComplete}
          onChange={(goalComplete) => update({ goalComplete })}
        />
      </TokenRow>
      <TokenRow
        title="Task completed"
        desc="When an agent finishes replying in a task without a running goal"
      >
        <TokenSwitch
          label="Task completed"
          on={notifications.taskComplete}
          onChange={(taskComplete) => update({ taskComplete })}
        />
      </TokenRow>
    </>
  );
}

function updateText(state: UpdateState, canInstall: boolean): string {
  switch (state.status) {
    case "idle":
      return canInstall ? "Updates install on their own" : "This copy can't install updates";
    case "checking":
      return "Checking for updates…";
    case "upToDate":
      return "Up to date";
    case "available":
      return `Patchwork ${state.version} is available`;
    case "downloading": {
      const progress = state.total
        ? `${Math.floor((state.downloaded / state.total) * 100)}%`
        : `${(state.downloaded / 2 ** 20).toFixed(1)} MB`;
      return `Downloading Patchwork ${state.version} · ${progress}`;
    }
    case "ready":
      return `Patchwork ${state.version} is installed. Restart to use it.`;
    case "failed":
      return state.message;
  }
}

/** The running version, where updating stands, and the action that moves it on. */
function UpdatesPane({ settings, updateSettings }: PaneProps) {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    const unlisten = onUpdateStatus((next) => {
      if (live) setStatus(next);
    });
    // An event that arrived first is newer than this reply.
    api
      .updateStatus()
      .then((next) => {
        if (live) setStatus((current) => current ?? next);
      })
      .catch((e: unknown) => setError(errorText(e)));
    return () => {
      live = false;
      void unlisten.then((stop) => stop());
    };
  }, []);
  if (!status) return error && <div className="py-2.5 text-danger">{error}</div>;
  const { state } = status;

  const run = (action: () => Promise<unknown>) => {
    setError(null);
    action().catch((e: unknown) => setError(errorText(e)));
  };
  let action: ReactNode;
  if (state.status === "ready") {
    action = (
      <button className="btn" onClick={() => run(restartToUpdate)}>
        Restart to update
      </button>
    );
  } else if (state.status === "available") {
    action = (
      <button className="btn" onClick={() => run(() => api.openUrl(RELEASES_URL))}>
        Download
      </button>
    );
  } else {
    const working = state.status === "checking" || state.status === "downloading";
    action = (
      <button className="btn" disabled={working} onClick={() => run(api.checkForUpdates)}>
        {state.status === "checking" ? "Checking…" : "Check for updates"}
      </button>
    );
  }
  return (
    <>
      <div className="flex items-center gap-3 border-b border-border py-2.5">
        <div className="min-w-0 flex-1">
          <div className="font-semibold">Patchwork {status.currentVersion}</div>
          <div
            className={`text-[length:var(--fs-sm)] ${state.status === "failed" ? "text-danger" : "text-fg-muted"}`}
          >
            {updateText(state, status.canInstall)}
          </div>
        </div>
        {action}
      </div>
      <TokenRow title="Check for updates every">
        <MinutesStepper
          label="Check for updates every"
          value={settings.updates.checkEveryMinutes}
          onChange={(checkEveryMinutes) =>
            void updateSettings((s) => ({ ...s, updates: { ...s.updates, checkEveryMinutes } }))
          }
        />
      </TokenRow>
      {error && <div className="py-2.5 text-[length:var(--fs-sm)] text-danger">{error}</div>}
    </>
  );
}

const CHECK_MINUTES = [10, 20, 30, 40, 50, 60];

/** Steps through `CHECK_MINUTES`. */
function MinutesStepper({
  label,
  value,
  onChange,
}: {
  label: string;
  value: number;
  onChange: (value: number) => void;
}) {
  const at = Math.max(0, CHECK_MINUTES.indexOf(value));
  const step =
    "grid h-6 w-6 cursor-pointer place-items-center border-none bg-transparent p-0 text-fg-muted hover:bg-hover hover:text-fg disabled:cursor-default disabled:opacity-40 disabled:hover:bg-transparent";
  return (
    <div
      role="group"
      aria-label={label}
      className="inline-flex items-center overflow-hidden rounded-[var(--radius-sm)] border border-border-strong"
    >
      <button
        className={step}
        aria-label="Less often"
        disabled={at === 0}
        onClick={() => onChange(CHECK_MINUTES[at - 1])}
      >
        <MinusIcon size={12} />
      </button>
      <span className="w-14 border-x border-border-strong text-center text-[length:var(--fs-sm)] tabular-nums">
        {CHECK_MINUTES[at] === 60 ? "1 hr" : `${CHECK_MINUTES[at]} min`}
      </span>
      <button
        className={step}
        aria-label="More often"
        disabled={at === CHECK_MINUTES.length - 1}
        onClick={() => onChange(CHECK_MINUTES[at + 1])}
      >
        <PlusIcon size={12} />
      </button>
    </div>
  );
}

/** Downloads and installs the Antigravity CLI into Patchwork's CLI folder, then re-checks it. */
function AntigravityInstall({ onInstalled }: { onInstalled: () => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const install = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.installAntigravity();
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

function AgentPane({ settings, updateSettings }: PaneProps) {
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

  useEffect(() => {
    let cancelled = false;
    for (const kind of CLI_KINDS) {
      api.detectAgent(kind).then(
        (s) => {
          if (!cancelled) setStatus((prev) => ({ ...prev, [kind]: s }));
        },
        () => {
          if (!cancelled) setStatus((prev) => ({ ...prev, [kind]: null }));
        },
      );
    }
    return () => {
      cancelled = true;
    };
  }, []);

  const connected = settings.agents;
  const toggle = (kind: AgentKind) =>
    void updateSettings((st) => ({
      ...st,
      agents: st.agents.includes(kind) ? st.agents.filter((k) => k !== kind) : [...st.agents, kind],
    }));
  const makeDefault = (kind: AgentKind) =>
    void updateSettings((st) => ({
      ...st,
      agents: [kind, ...st.agents.filter((k) => k !== kind)],
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
          <div key={kind} className="flex items-center gap-3 border-b border-border py-2.5">
            <ProviderLogos kind={kind} size={20} />
            <div className="min-w-0 flex-1">
              <div className="font-semibold">{AGENT_LABELS[kind]}</div>
              <div className="text-[length:var(--fs-sm)] text-fg-muted">{describeAgent(s)}</div>
            </div>
            <span className={`badge${s?.installed ? " badge-success" : ""}`}>{badge}</span>
            {kind === "antigravity" && !s?.installed && s !== undefined && (
              <AntigravityInstall onInstalled={refresh} />
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
              className="toggle"
              role="switch"
              aria-label={`Connect ${AGENT_LABELS[kind]}`}
              aria-checked={on}
              disabled={locked}
              onClick={() => toggle(kind)}
            />
          </div>
        );
      })}
      {connected.length > 1 && (
        <div className="flex items-center gap-3 border-b border-border py-2.5">
          <div className="min-w-0 flex-1">
            <div className="font-semibold">Default for new tasks</div>
            <div className="text-[length:var(--fs-sm)] text-fg-muted">
              You can still pick an agent per task in the composer
            </div>
          </div>
          <div className="inline-flex overflow-hidden rounded-[var(--radius-sm)] border border-border-strong">
            {/* Fixed order: choosing a default reorders `agents`, not the buttons. */}
            {AGENT_KINDS.filter((kind) => connected.includes(kind)).map((kind) => (
              <button
                key={kind}
                className={`grid h-6 w-auto cursor-pointer place-items-center border-none px-2.5 py-0 text-[length:var(--fs-sm)] [&+&]:border-l [&+&]:border-border-strong ${connected[0] === kind ? "bg-selected text-fg" : "bg-transparent text-fg-muted hover:bg-hover hover:text-fg"}`}
                onClick={() => makeDefault(kind)}
              >
                {AGENT_LABELS[kind]}
              </button>
            ))}
          </div>
        </div>
      )}
    </>
  );
}

function ImportsPane({
  settings,
  updateSettings,
  onImported,
}: PaneProps & { onImported: () => void }) {
  const [scans, setScans] = useState<Partial<Record<ImportSource, ImportScan | null>>>({});
  const [busy, setBusy] = useState<ImportSource | null>(null);
  const [error, setError] = useState<string | null>(null);
  const sources = importSourcesFor(AGENT_KINDS.filter((k) => settings.agents.includes(k)));
  const key = sources.join(",");

  const scan = useCallback((kind: ImportSource) => {
    api.scanImports(kind).then(
      (s) => setScans((prev) => ({ ...prev, [kind]: s })),
      () => setScans((prev) => ({ ...prev, [kind]: null })),
    );
  }, []);

  // `key` stands in for `sources`, which is a new array on every render.
  useEffect(() => {
    for (const kind of sources) scan(kind);
  }, [key, scan]);

  const importFrom = async (kind: ImportSource) => {
    setBusy(kind);
    setError(null);
    try {
      await api.importThreads([kind]);
      // Importing adds projects on the backend; pick them up before saving settings again.
      await updateSettings(await api.getSettings());
      onImported();
      scan(kind);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <>
      {sources.map((kind) => {
        const s = scans[kind];
        // T3 Code is only offered to people who use it.
        if (kind === "t3" && !(s && s.found > 0)) return null;
        const desc =
          s === undefined
            ? "Looking for saved conversations…"
            : s === null
              ? "Couldn't read saved conversations"
              : s.new > 0
                ? `${s.new} of ${s.found} conversations not imported yet`
                : s.found > 0
                  ? `All ${s.found} conversations imported`
                  : "No saved conversations";
        return (
          <div key={kind} className="flex items-center gap-3 border-b border-border py-2.5">
            <ProviderLogos kind={kind} size={20} />
            <div className="min-w-0 flex-1">
              <div className="font-semibold">{SOURCE_LABELS[kind]}</div>
              <div className="text-[length:var(--fs-sm)] text-fg-muted">{desc}</div>
              {s && s.subagents > 0 && (
                <div className="text-[length:var(--fs-sm)] text-fg-muted">
                  {s.subagents} sub-agent {s.subagents === 1 ? "conversation" : "conversations"}{" "}
                  hidden
                </div>
              )}
            </div>
            <button
              className="btn"
              disabled={busy !== null || !s || s.new === 0}
              onClick={() => void importFrom(kind)}
            >
              {busy === kind ? "Importing…" : "Import"}
            </button>
          </div>
        );
      })}
      {error && <div className="text-[length:var(--fs-sm)] text-danger">{error}</div>}
      <TokenRow title="Import log" desc="Writes each scan and import to logs/imports.log">
        <TokenSwitch
          label="Import log"
          on={!settings.importLogDisabled}
          onChange={(on) => void updateSettings((s) => ({ ...s, importLogDisabled: !on }))}
        />
      </TokenRow>
    </>
  );
}

const basename = (path: string) => path.split("/").pop() ?? path;

/** How the agents' shell is chosen. Shells come from /etc/shells; a saved shell that is no
 * longer listed stays in the menu so the choice is still visible. */
function EnvironmentPane({ settings, updateSettings }: PaneProps) {
  const [choices, setChoices] = useState<ShellChoices>({ shells: [], default: null });
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.listShells().then(setChoices, (e) => setError(errorText(e)));
  }, []);

  const { shells, default: fallback } = choices;
  const shell = settings.shell ?? "";
  const options = shell && !shells.includes(shell) ? [shell, ...shells] : shells;
  const choose = (path: string | null) => void updateSettings((s) => ({ ...s, shell: path }));
  return (
    <>
      <TokenRow title="Shell" desc="The shell agents run commands in">
        <Dropdown
          alignEnd
          trigger={({ open, toggle }) => (
            <button
              className={CHIP_CLASS}
              aria-haspopup="menu"
              aria-expanded={open}
              aria-label={`Shell: ${shell ? basename(shell) : "System default"}`}
              title={shell || `Follows your login shell${fallback ? ` (${fallback})` : ""}`}
              onClick={toggle}
            >
              <span>{shell ? basename(shell) : "System default"}</span>
              <CaretDownIcon size={14} />
            </button>
          )}
        >
          {(close) => (
            <div className="max-h-[240px] overflow-y-auto">
              {[null, ...options].map((path) => (
                <button
                  key={path ?? "default"}
                  role="menuitemradio"
                  aria-checked={path === (shell || null)}
                  title={path ?? undefined}
                  className={`flex w-full min-w-[160px] cursor-pointer items-center gap-1.5 rounded-[var(--radius-sm)] border-none bg-transparent px-2 py-[5px] text-left whitespace-nowrap text-fg hover:bg-hover ${path === (shell || null) ? "bg-selected-muted" : ""}`}
                  onClick={() => {
                    close();
                    choose(path);
                  }}
                >
                  {path
                    ? basename(path)
                    : `System default${fallback ? ` (${basename(fallback)})` : ""}`}
                </button>
              ))}
            </div>
          )}
        </Dropdown>
      </TokenRow>
      {error && <div className="text-[length:var(--fs-sm)] text-danger">{error}</div>}
    </>
  );
}

/** How a skills folder is laid out: one folder per skill, named like the skill, with its
 * SKILL.md and anything the skill refers to. */
const SKILLS_EXAMPLE = `skills/
├── release-notes/
│   ├── SKILL.md
│   └── template.md
└── review-ui/
    ├── SKILL.md
    └── scripts/
        └── measure.sh

release-notes/SKILL.md
---
name: release-notes
description: Draft release notes from the commits since the last tag
---
Read template.md, then list the commits since the last tag…`;

/** The skills folder every agent loads, and the skills in it. */
function SkillsPane({ settings, updateSettings }: PaneProps) {
  const [view, setView] = useState<SkillsView | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => api.listSkills().then(setView, (e) => setError(errorText(e))), []);
  useEffect(() => {
    void load();
  }, [load]);

  async function choose(dir: string | null) {
    setError(null);
    await updateSettings((s) => ({ ...s, skillsDir: dir }));
    await load();
  }

  async function toggle(name: string, on: boolean) {
    setView(
      (v) =>
        v && { ...v, skills: v.skills.map((s) => (s.name === name ? { ...s, enabled: on } : s)) },
    );
    await updateSettings((s) => ({
      ...s,
      disabledSkills: on
        ? s.disabledSkills.filter((n) => n !== name)
        : [...s.disabledSkills.filter((n) => n !== name), name],
    }));
  }

  async function pick() {
    const picked = await open({ directory: true, defaultPath: view?.dir });
    if (typeof picked === "string") await choose(picked);
  }

  return (
    <>
      <TokenRow title="Folder" desc={view?.dir}>
        {settings.skillsDir && (
          <button className="btn" onClick={() => void choose(null)}>
            Use default
          </button>
        )}
        <button className="btn" onClick={() => void pick()}>
          Change…
        </button>
        <button
          className="btn"
          onClick={() => api.openSkillsFolder().catch((e) => setError(errorText(e)))}
        >
          Open
        </button>
      </TokenRow>
      {error && <div className="mt-2 text-[length:var(--fs-sm)] text-danger">{error}</div>}
      {view?.skills.length === 0 && (
        <div className="px-3 py-2 text-[length:var(--fs-sm)] text-fg-subtle">No skills yet</div>
      )}
      {view?.skills.map((skill) => (
        <div
          key={skill.path}
          className="flex items-center gap-3 border-b border-border py-2.5"
          title={skill.description ?? skill.path}
        >
          <div className="min-w-0 flex-1 truncate font-[family-name:var(--font-mono)]">
            ${skill.name}
          </div>
          <TokenSwitch
            label={`$${skill.name}`}
            on={skill.enabled}
            onChange={(on) => void toggle(skill.name, on)}
          />
        </div>
      ))}
      <div className="mt-5 mb-1.5 font-semibold">Example</div>
      <pre className="m-0 overflow-x-auto rounded-[var(--radius-sm)] border border-border-strong bg-editor px-3 py-2.5 font-[family-name:var(--font-mono)] text-[length:var(--fs-xs)] leading-relaxed text-fg-muted">
        {SKILLS_EXAMPLE}
      </pre>
    </>
  );
}

function ProjectsPane({ settings, updateSettings }: PaneProps) {
  const [error, setError] = useState<string | null>(null);

  async function remove(id: string) {
    setError(null);
    try {
      await api.removeProject(id);
      await syncProjects(updateSettings);
    } catch (e) {
      setError(errorText(e));
    }
  }

  return (
    <>
      {settings.projects.length === 0 && (
        <div className="px-3 py-2 text-[length:var(--fs-sm)] text-fg-subtle">No projects yet</div>
      )}
      {settings.projects.map((p) => (
        <div key={p.id} className="flex items-center gap-2 border-b border-border py-2">
          <div className="min-w-0 flex-1">
            <div className="font-semibold">{p.name}</div>
            <div className="overflow-hidden text-ellipsis whitespace-nowrap font-[family-name:var(--font-mono)] text-[length:var(--fs-xs)] text-fg-muted">
              {p.path}
            </div>
          </div>
          <button
            className="btn-icon"
            title="Remove project"
            aria-label="Remove project"
            onClick={() => void remove(p.id)}
          >
            <TrashIcon size={14} />
          </button>
        </div>
      ))}
      {error && <div className="mt-2 text-[length:var(--fs-sm)] text-danger">{error}</div>}
      <div className="mt-4">
        <button
          className="btn"
          onClick={() => void updateSettings((s) => ({ ...s, onboarded: false }))}
        >
          Run setup again
        </button>
      </div>
    </>
  );
}
