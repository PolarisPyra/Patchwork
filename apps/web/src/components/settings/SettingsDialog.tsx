import { useCallback, useEffect, useState, type ReactNode } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
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
  MagnifyingGlassIcon,
  MinusIcon,
  PaletteIcon,
  PlugIcon,
  PlugsConnectedIcon,
  PlusIcon,
  RobotIcon,
  ShieldCheckIcon,
  ScrollIcon,
  TerminalIcon,
  TerminalWindowIcon,
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
  type LoggingSettings,
  type ModelOption,
  type NotificationSettings,
  type Settings,
  type ShellChoices,
  type TerminalSettings,
  defaultTerminalSettings,
  type SkillsView,
  type Theme,
  type TokenSettings,
  type UpdateState,
  type UpdateStatus,
} from "~/api";
import ProviderLogos from "~/components/ProviderLogos";
import LogLevelPicker from "~/components/panel/LogLevelPicker";
import type { UpdateSettings } from "~/app/App";
import { syncProjects } from "~/lib/projects";
import { formatTokens } from "~/lib/tokens";
import { AGENT_KINDS, CLI_KINDS, describeAgent, errorText } from "~/lib/util";
import logoManifest from "../../../../../assets/logos/manifest.json";
import changelog from "../../../../../CHANGELOG.md?raw";
import Markdown from "~/components/chat/Markdown";
import { CHIP_CLASS } from "~/components/chat/ModelPicker";
import Dropdown from "~/components/workspace/Dropdown";
import KeybindingsPane from "~/components/settings/KeybindingsPane";
import ApiProvidersPane from "~/components/settings/ApiProvidersPane";
import McpServersPane, { Badge, chip, home, INPUT } from "~/components/settings/McpServersPane";
import PermissionsPane, { type PermissionsFocus } from "~/components/settings/PermissionsPane";

type Section =
  | "appearance"
  | "agent"
  | "permissions"
  | "api"
  | "mcp"
  | "tokens"
  | "notifications"
  | "imports"
  | "environment"
  | "terminal"
  | "skills"
  | "projects"
  | "shortcuts"
  | "logging"
  | "updates";

/** The panes, in groups under a heading, so the list reads in a few short runs. */
const GROUPS: { label: string; sections: { id: Section; label: string; Icon: Icon }[] }[] = [
  {
    label: "General",
    sections: [
      { id: "appearance", label: "Appearance", Icon: PaletteIcon },
      { id: "notifications", label: "Notifications", Icon: BellIcon },
      { id: "shortcuts", label: "Shortcuts", Icon: KeyboardIcon },
      { id: "updates", label: "Updates", Icon: ArrowCircleUpIcon },
    ],
  },
  {
    label: "Agents",
    sections: [
      { id: "agent", label: "CLI Providers", Icon: RobotIcon },
      { id: "api", label: "API Providers", Icon: PlugsConnectedIcon },
      { id: "mcp", label: "MCP Servers", Icon: PlugIcon },
      { id: "skills", label: "Skills", Icon: BooksIcon },
      { id: "permissions", label: "Permissions", Icon: ShieldCheckIcon },
      { id: "tokens", label: "AI Tokenomics", Icon: GaugeIcon },
    ],
  },
  {
    label: "Projects",
    sections: [
      { id: "projects", label: "Projects", Icon: FolderSimpleIcon },
      { id: "imports", label: "Imports", Icon: DownloadSimpleIcon },
    ],
  },
  {
    label: "System",
    sections: [
      { id: "environment", label: "Environment", Icon: TerminalIcon },
      { id: "terminal", label: "Terminal", Icon: TerminalWindowIcon },
      { id: "logging", label: "Logging", Icon: ScrollIcon },
    ],
  },
];

const SECTIONS = GROUPS.flatMap((g) => g.sections);

interface PaneProps {
  settings: Settings;
  updateSettings: UpdateSettings;
}

interface SettingsDialogProps extends PaneProps {
  /** Opens on Settings → Permissions, with this scope and rule picked out. */
  focus?: PermissionsFocus | null;
  /** Called after conversations were imported, so threads can be reloaded. */
  onImported: () => void;
  onClose: () => void;
}

export default function SettingsDialog({
  settings,
  updateSettings,
  focus,
  onImported,
  onClose,
}: SettingsDialogProps) {
  const [section, setSection] = useState<Section>(focus ? "permissions" : "appearance");
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
        <nav className="flex w-[180px] flex-none flex-col overflow-y-auto border-r border-border bg-panel px-2 py-2.5">
          {GROUPS.map((group) => (
            <div key={group.label} className="flex flex-col gap-0.5 [&+&]:mt-3">
              <div className="px-2 pb-0.5 text-[length:var(--fs-xs)] font-semibold text-fg-subtle">
                {group.label}
              </div>
              {group.sections.map(({ id, label, Icon }) => (
                <button
                  key={id}
                  className={`flex h-8 cursor-pointer items-center gap-2.5 rounded-[var(--radius-sm)] border-none px-2 text-left text-fg ${section === id ? "bg-selected hover:bg-[color-mix(in_srgb,var(--bg-selected),var(--text)_8%)]" : "bg-transparent hover:bg-hover"}`}
                  onClick={() => setSection(id)}
                >
                  <Icon size={18} className="flex-none" />
                  {label}
                </button>
              ))}
            </div>
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
            {section === "permissions" && <PermissionsPane {...paneProps} focus={focus ?? null} />}
            {section === "api" && <ApiProvidersPane updateSettings={updateSettings} />}
            {section === "mcp" && (
              <McpServersPane appIcon={LOGO_URLS[settings.logo ?? logoManifest.default]} />
            )}
            {section === "tokens" && <TokensPane {...paneProps} />}
            {section === "notifications" && <NotificationsPane {...paneProps} />}
            {section === "imports" && <ImportsPane {...paneProps} onImported={onImported} />}
            {section === "environment" && <EnvironmentPane {...paneProps} />}
            {section === "terminal" && <TerminalPane {...paneProps} />}
            {section === "skills" && <SkillsPane {...paneProps} />}
            {section === "projects" && <ProjectsPane {...paneProps} />}
            {section === "shortcuts" && <KeybindingsPane />}
            {section === "logging" && <LoggingPane {...paneProps} />}
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
export function TokenRow({
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
      <TokenRow title="Generate chat and task titles">
        <TokenSwitch
          label="Generate chat and task titles"
          on={tokens.generateTitles}
          onChange={(generateTitles) => update({ generateTitles })}
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
      {status.notes && state.status !== "upToDate" && (
        <div className="border-b border-border pt-1 pb-3">
          <Markdown text={releaseNotes(status.notes)} />
        </div>
      )}
      <div className="pt-1">
        <Markdown text={LATEST_RELEASE} />
      </div>
    </>
  );
}

/** A release's section of CHANGELOG.md, its `## [0.2.0] - 2026-01-02` heading read as
 * `## 0.2.0 · 2026-01-02`. */
function releaseNotes(section: string): string {
  return section.replace(/^## \[(.+?)\] - /, "## $1 · ");
}

/** The newest release's section of CHANGELOG.md as it was when this copy was built. */
const LATEST_RELEASE = (() => {
  const start = changelog.indexOf("\n## [") + 1;
  const end = changelog.indexOf("\n## [", start);
  return releaseNotes(changelog.slice(start, end === -1 ? undefined : end));
})();

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
    </>
  );
}

const LOGS: { key: keyof LoggingSettings; title: string; desc: string }[] = [
  { key: "app", title: "Main", desc: "Patchwork's own messages, in main.log" },
  {
    key: "window",
    title: "Window",
    desc: "The window's errors and warnings, in window/renderer.log",
  },
  { key: "agents", title: "Agents", desc: "Each agent's runs, in agents/" },
  { key: "imports", title: "Imports", desc: "Each scan and import, in imports.log" },
];

/** The logs each launch writes to its own folder in the config folder's `logs`, as VS Code's,
 * which people can send with a bug report. */
function LoggingPane({ settings, updateSettings }: PaneProps) {
  const [error, setError] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);

  async function exportLogs() {
    const stamp = new Date().toISOString().slice(0, 19).replace(/[-:]/g, "");
    const path = await save({
      defaultPath: `patchwork-logs-${stamp}.zip`,
      filters: [{ name: "Zip", extensions: ["zip"] }],
    });
    if (!path) return;
    setError(null);
    setExporting(true);
    try {
      await api.exportLogs(path);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setExporting(false);
    }
  }

  return (
    <>
      {LOGS.map((log) => (
        <TokenRow key={log.key} title={log.title} desc={log.desc}>
          <LogLevelPicker
            label={log.title}
            value={settings.logging[log.key]}
            onChange={(level) =>
              void updateSettings((s) => ({ ...s, logging: { ...s.logging, [log.key]: level } }))
            }
          />
        </TokenRow>
      ))}
      <TokenRow title="Log folder">
        <button className="btn" disabled={exporting} onClick={() => void exportLogs()}>
          {exporting ? "Exporting…" : "Export…"}
        </button>
        <button
          className="btn"
          onClick={() => api.openLogsFolder().catch((e) => setError(errorText(e)))}
        >
          Open
        </button>
      </TokenRow>
      {error && <div className="mt-2 text-[length:var(--fs-sm)] text-danger">{error}</div>}
    </>
  );
}

/** Two to five choices side by side, as the theme picker. */
export function Segmented<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: { value: T; label: string }[];
  onChange: (value: T) => void;
}) {
  return (
    <div
      role="radiogroup"
      aria-label={label}
      className="inline-flex overflow-hidden rounded-[var(--radius-sm)] border border-border-strong"
    >
      {options.map((o) => (
        <button
          key={o.value}
          role="radio"
          aria-checked={value === o.value}
          className={`grid h-6 w-auto cursor-pointer place-items-center border-none px-2.5 py-0 text-[length:var(--fs-sm)] whitespace-nowrap [&+&]:border-l [&+&]:border-border-strong ${value === o.value ? "bg-selected text-fg" : "bg-transparent text-fg-muted hover:bg-hover hover:text-fg"}`}
          onClick={() => onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** A text field that saves when it loses focus or on Enter. */
export function TextField({
  label,
  value,
  placeholder,
  width = "w-[220px]",
  onSave,
}: {
  label: string;
  value: string;
  placeholder?: string;
  width?: string;
  onSave: (value: string) => void;
}) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  return (
    <input
      className={`input ${width} text-[length:var(--fs-sm)]`}
      aria-label={label}
      value={draft}
      placeholder={placeholder}
      spellCheck={false}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={() => draft !== value && onSave(draft)}
      onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
    />
  );
}

/** A number field that saves a valid number when it loses focus or on Enter. */
function NumberField({
  label,
  value,
  step = 1,
  min,
  max,
  onSave,
}: {
  label: string;
  value: number;
  step?: number;
  min?: number;
  max?: number;
  onSave: (value: number) => void;
}) {
  return (
    <TextField
      label={label}
      value={String(value)}
      width="w-[80px]"
      onSave={(text) => {
        const n = Number(text);
        if (text.trim() === "" || !Number.isFinite(n)) return;
        const clamped = Math.min(max ?? n, Math.max(min ?? n, n));
        onSave(step >= 1 ? Math.round(clamped) : clamped);
      }}
    />
  );
}

const isMacPlatform = /Mac|iPhone|iPad/.test(navigator.platform);

/** The integrated terminal, with VS Code's `terminal.integrated.*` settings. The shell is the
 * Environment pane's. */
function TerminalPane({ settings, updateSettings }: PaneProps) {
  const t = settings.terminal;
  const set = (patch: Partial<TerminalSettings>) =>
    void updateSettings((s) => ({ ...s, terminal: { ...s.terminal, ...patch } }));
  const heading = (text: string) => (
    <h3 className="m-0 mt-4 mb-0.5 text-[length:var(--fs-sm)] font-medium text-fg-subtle first:mt-0">
      {text}
    </h3>
  );
  return (
    <>
      {heading("Shell")}
      <TokenRow title="Shell arguments">
        <TextField
          label="Shell arguments"
          value={t.shellArgs?.join(" ") ?? ""}
          placeholder={isMacPlatform ? "-l" : "None"}
          onSave={(text) => set({ shellArgs: text.trim() ? text.trim().split(/\s+/) : null })}
        />
      </TokenRow>
      <TokenRow title="Starting folder">
        <TextField
          label="Starting folder"
          value={t.cwd ?? ""}
          placeholder="Project folder"
          onSave={(text) => set({ cwd: text.trim() || null })}
        />
      </TokenRow>
      <TokenRow title="Inherit environment" desc="Start from Patchwork's environment">
        <TokenSwitch
          label="Inherit environment"
          on={t.inheritEnv}
          onChange={(on) => set({ inheritEnv: on })}
        />
      </TokenRow>
      <TokenRow title="UTF-8 locale" desc="Sets LANG; Auto only when it is not UTF-8">
        <Segmented
          label="UTF-8 locale"
          value={t.detectLocale}
          options={[
            { value: "auto", label: "Auto" },
            { value: "on", label: "On" },
            { value: "off", label: "Off" },
          ]}
          onChange={(detectLocale) => set({ detectLocale })}
        />
      </TokenRow>
      <EnvEditor env={t.env} onChange={(env) => set({ env })} />

      {heading("Appearance")}
      <TokenRow title="Font">
        <TextField
          label="Font"
          value={t.fontFamily ?? ""}
          placeholder="App monospace font"
          onSave={(text) => set({ fontFamily: text.trim() || null })}
        />
      </TokenRow>
      <TokenRow title="Font size">
        <NumberField
          label="Font size"
          value={t.fontSize}
          min={6}
          max={100}
          onSave={(fontSize) => set({ fontSize })}
        />
      </TokenRow>
      <TokenRow title="Line height">
        <NumberField
          label="Line height"
          value={t.lineHeight}
          step={0.1}
          min={1}
          onSave={(lineHeight) => set({ lineHeight })}
        />
      </TokenRow>
      <TokenRow title="Letter spacing">
        <NumberField
          label="Letter spacing"
          value={t.letterSpacing}
          step={0.5}
          onSave={(letterSpacing) => set({ letterSpacing })}
        />
      </TokenRow>
      <TokenRow title="Font weight">
        <TextField
          label="Font weight"
          value={t.fontWeight}
          width="w-[80px]"
          onSave={(text) => text.trim() && set({ fontWeight: text.trim() })}
        />
      </TokenRow>
      <TokenRow title="Bold font weight">
        <TextField
          label="Bold font weight"
          value={t.fontWeightBold}
          width="w-[80px]"
          onSave={(text) => text.trim() && set({ fontWeightBold: text.trim() })}
        />
      </TokenRow>
      <TokenRow title="Cursor">
        <Segmented
          label="Cursor style"
          value={t.cursorStyle}
          options={[
            { value: "block", label: "Block" },
            { value: "line", label: "Line" },
            { value: "underline", label: "Underline" },
          ]}
          onChange={(cursorStyle) => set({ cursorStyle })}
        />
      </TokenRow>
      {t.cursorStyle === "line" && (
        <TokenRow title="Cursor width">
          <NumberField
            label="Cursor width"
            value={t.cursorWidth}
            min={1}
            onSave={(cursorWidth) => set({ cursorWidth })}
          />
        </TokenRow>
      )}
      <TokenRow title="Cursor blinking">
        <TokenSwitch
          label="Cursor blinking"
          on={t.cursorBlinking}
          onChange={(cursorBlinking) => set({ cursorBlinking })}
        />
      </TokenRow>
      <TokenRow title="Minimum contrast ratio">
        <NumberField
          label="Minimum contrast ratio"
          value={t.minimumContrastRatio}
          step={0.5}
          min={1}
          max={21}
          onSave={(minimumContrastRatio) => set({ minimumContrastRatio })}
        />
      </TokenRow>
      <TokenRow title="Bold text in bright colors">
        <TokenSwitch
          label="Bold text in bright colors"
          on={t.drawBoldTextInBrightColors}
          onChange={(drawBoldTextInBrightColors) => set({ drawBoldTextInBrightColors })}
        />
      </TokenRow>

      {heading("Behavior")}
      <TokenRow title="Scrollback" desc="Lines kept above the screen">
        <NumberField
          label="Scrollback"
          value={t.scrollback}
          min={0}
          onSave={(scrollback) => set({ scrollback })}
        />
      </TokenRow>
      <TokenRow title="Copy on selection">
        <TokenSwitch
          label="Copy on selection"
          on={t.copyOnSelection}
          onChange={(copyOnSelection) => set({ copyOnSelection })}
        />
      </TokenRow>
      <TokenRow title="Right click">
        <Segmented
          label="Right click"
          value={t.rightClickBehavior}
          options={[
            { value: "default", label: "Menu" },
            { value: "copyPaste", label: "Copy or paste" },
            { value: "paste", label: "Paste" },
            { value: "selectWord", label: "Select word" },
            { value: "nothing", label: "Nothing" },
          ]}
          onChange={(rightClickBehavior) => set({ rightClickBehavior })}
        />
      </TokenRow>
      <TokenRow title="Alt-click moves the cursor">
        <TokenSwitch
          label="Alt-click moves the cursor"
          on={t.altClickMovesCursor}
          onChange={(altClickMovesCursor) => set({ altClickMovesCursor })}
        />
      </TokenRow>
      {isMacPlatform && (
        <TokenRow title="Option is Meta">
          <TokenSwitch
            label="Option is Meta"
            on={t.macOptionIsMeta}
            onChange={(macOptionIsMeta) => set({ macOptionIsMeta })}
          />
        </TokenRow>
      )}
      <TokenRow title="Smooth scrolling">
        <TokenSwitch
          label="Smooth scrolling"
          on={t.smoothScrolling}
          onChange={(smoothScrolling) => set({ smoothScrolling })}
        />
      </TokenRow>
      <TokenRow title="Scroll speed">
        <NumberField
          label="Scroll speed"
          value={t.mouseWheelScrollSensitivity}
          step={0.5}
          min={0.1}
          onSave={(mouseWheelScrollSensitivity) => set({ mouseWheelScrollSensitivity })}
        />
      </TokenRow>
      <TokenRow title="Fast scroll speed" desc="While Alt is held">
        <NumberField
          label="Fast scroll speed"
          value={t.fastScrollSensitivity}
          step={0.5}
          min={0.1}
          onSave={(fastScrollSensitivity) => set({ fastScrollSensitivity })}
        />
      </TokenRow>
      <div className="mt-4">
        <button className="btn" onClick={() => set(defaultTerminalSettings())}>
          Reset to defaults
        </button>
      </div>
    </>
  );
}

/** The terminal's environment variables. A removed variable is taken out of the shell's
 * environment, as `null` in VS Code's `terminal.integrated.env`. */
function EnvEditor({
  env,
  onChange,
}: {
  env: Record<string, string | null>;
  onChange: (env: Record<string, string | null>) => void;
}) {
  const entries = Object.entries(env);
  const replace = (index: number, name: string, value: string | null) =>
    onChange(
      Object.fromEntries(
        entries.map((entry, i) => (i === index ? [name, value] : entry)).filter(([n]) => n),
      ),
    );
  return (
    <div className="border-b border-border py-2.5">
      <div className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <div className="font-semibold">Environment variables</div>
          <div className="text-[length:var(--fs-sm)] text-fg-muted">
            {"Values can use ${env:NAME} and ${workspaceFolder}"}
          </div>
        </div>
        <button
          className="btn"
          onClick={() => {
            let name = "NAME";
            for (let n = 2; name in env; n++) name = `NAME_${n}`;
            onChange({ ...env, [name]: "" });
          }}
        >
          Add
        </button>
      </div>
      {entries.map(([name, value], i) => (
        <div key={i} className="mt-1.5 flex items-center gap-1.5">
          <TextField
            label="Variable name"
            value={name}
            width="w-[160px]"
            onSave={(next) => replace(i, next.trim(), value)}
          />
          <TextField
            label={`${name} value`}
            value={value ?? ""}
            width="min-w-0 flex-1"
            placeholder={value === null ? "Removed from the shell" : ""}
            onSave={(next) => replace(i, name, next)}
          />
          <button
            className={`btn ${value === null ? "bg-selected-muted" : ""}`}
            aria-pressed={value === null}
            title="Take this variable out of the shell's environment"
            onClick={() => replace(i, name, value === null ? "" : null)}
          >
            Remove from shell
          </button>
          <button
            className="btn-icon"
            aria-label={`Delete ${name}`}
            onClick={() => onChange(Object.fromEntries(entries.filter((_, j) => j !== i)))}
          >
            <XIcon size={14} />
          </button>
        </div>
      ))}
    </div>
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

type SkillFilter = "on" | "off";

/** The skills folder every agent loads, and the skills in it, kept up to date as it changes. */
function SkillsPane({ settings, updateSettings }: PaneProps) {
  const [view, setView] = useState<SkillsView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<SkillFilter | null>(null);
  const [query, setQuery] = useState("");

  // A folder that can't be watched still lists its skills.
  const watch = useCallback(
    () =>
      api.watchSkills(setView).then(setView, (e) => {
        setError(errorText(e));
        return api.listSkills().then(setView);
      }),
    [],
  );
  useEffect(() => {
    void watch();
    return () => void api.unwatchSkills();
  }, [watch]);

  async function choose(dir: string | null) {
    setError(null);
    await updateSettings((s) => ({ ...s, skillsDir: dir }));
    await watch();
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

  if (view === null) return null;

  const run = (action: Promise<void>) => {
    setError(null);
    action.catch((e) => setError(errorText(e)));
  };
  const q = query.trim().toLowerCase();
  const on = view.skills.filter((s) => s.enabled).length;
  const counts = { on, off: view.skills.length - on };
  const shown = view.skills.filter(
    (s) =>
      (!filter || s.enabled === (filter === "on")) &&
      (!q || `${s.name} ${s.description ?? ""}`.toLowerCase().includes(q)),
  );

  return (
    <>
      <div className="flex items-center gap-1.5 pb-3">
        <button
          className={chip(filter === null)}
          aria-pressed={filter === null}
          onClick={() => setFilter(null)}
        >
          All <span className="text-fg-muted">{view.skills.length}</span>
        </button>
        {(["on", "off"] as const).map((f) => (
          <button
            key={f}
            className={chip(filter === f)}
            aria-pressed={filter === f}
            onClick={() => setFilter(filter === f ? null : f)}
          >
            {f === "on" ? "On" : "Off"} <span className="text-fg-muted">{counts[f]}</span>
          </button>
        ))}
        <label className="relative ml-auto w-52">
          <MagnifyingGlassIcon
            size={13}
            className="pointer-events-none absolute top-1/2 left-2 -translate-y-1/2 text-fg-muted"
          />
          <input
            className={`${INPUT} pl-7`}
            value={query}
            placeholder="Search skills"
            spellCheck={false}
            onChange={(e) => setQuery(e.target.value)}
          />
        </label>
      </div>
      <div className="grid grid-cols-[repeat(auto-fill,minmax(196px,1fr))] gap-2">
        {shown.map((skill) => (
          <div
            key={skill.path}
            className={`flex min-h-[124px] min-w-0 flex-col gap-2.5 rounded-[var(--radius-md)] border border-border p-3 text-fg ${skill.enabled ? "" : "opacity-55"}`}
          >
            <div className="flex w-full min-w-0 items-center gap-2.5">
              <Badge name={skill.name} size={32} />
              <div className="min-w-0 flex-1">
                <div className="truncate font-semibold">${skill.name}</div>
                <div className="truncate font-mono text-[length:var(--fs-xs)] text-fg-muted">
                  {skill.path.startsWith(`${view.dir}/`)
                    ? skill.path.slice(view.dir.length + 1)
                    : home(skill.path)}
                </div>
              </div>
              <button className="btn flex-none" onClick={() => run(api.openSkill(skill.path))}>
                Edit
              </button>
            </div>
            <div className="line-clamp-2 flex-1 text-[length:var(--fs-sm)] text-fg-muted">
              {skill.description}
            </div>
            <div className="flex justify-end">
              <TokenSwitch
                label={`$${skill.name}`}
                on={skill.enabled}
                onChange={(on) => void toggle(skill.name, on)}
              />
            </div>
          </div>
        ))}
        {!q && !filter && (
          <button
            className="flex min-h-[124px] cursor-pointer flex-col items-center justify-center gap-1.5 rounded-[var(--radius-md)] border border-dashed border-border-strong bg-transparent text-fg-muted hover:border-accent hover:text-fg"
            onClick={() => run(api.openSkillsFolder())}
          >
            <FolderSimpleIcon size={20} />
            <span className="text-[length:var(--fs-sm)]">Open folder</span>
          </button>
        )}
      </div>
      {(q || filter) && shown.length === 0 && (
        <div className="py-6 text-center text-fg-muted">
          {q ? `Nothing matches “${query.trim()}”` : "No skills"}
        </div>
      )}
      {error && <div className="mt-2 text-[length:var(--fs-sm)] text-danger">{error}</div>}
      <div className="mt-3">
        <TokenRow title="Folder" desc={view.dir}>
          {settings.skillsDir && (
            <button className="btn" onClick={() => void choose(null)}>
              Use default
            </button>
          )}
          <button className="btn" onClick={() => void pick()}>
            Change…
          </button>
        </TokenRow>
      </div>
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
