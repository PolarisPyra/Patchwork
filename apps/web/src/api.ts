import type { AccessMode, AgentEvent, AgentEventEnvelope, AgentKind, AgentStatus, ApiModel, ApiProvider, Approval, ApprovalStatus, Attachment, AttachmentKind, BackgroundTask, CodexFeature, CodexInfo, CodexModel, Effect, FileChange, FileMedia, Folder, Goal, GoalStatus, ImportScan, ImportSource, Integrations, LogChunk, LoggingSettings, LogLevel, McpServer, Message as WireMessage, MessageStatus, ModelOption, NotificationSettings, Part, PermissionKind, PermissionRule, PermissionScope, PermissionSettings, PlanStep, Plugin, PluginApp, PluginHook, PluginHookHandler, Project, ProjectChanged, ProviderSlashCommand, ProviderUsage, Question, Refusal, Role, RuleRef, RunOutcome, Schedule, ScheduledRun, ScheduledTask, ScopeRules, Screenshot, Settings, ShellChoices, Skill, StepStatus, SubAgent, TerminalSettings, Theme, Thread as WireThread, TokenSettings, TokenUsage, ToolCall, TurnFile, TurnFileState, TurnTrees, UsageWindow, Weekday } from "~/bindings";
export type { AccessMode, AgentEvent, AgentEventEnvelope, AgentKind, AgentStatus, ApiModel, ApiProvider, Approval, ApprovalStatus, Attachment, AttachmentKind, BackgroundTask, CodexFeature, CodexInfo, CodexModel, Effect, FileChange, FileMedia, Folder, Goal, GoalStatus, ImportScan, ImportSource, Integrations, LogChunk, LoggingSettings, LogLevel, McpServer, MessageStatus, ModelOption, NotificationSettings, Part, PermissionKind, PermissionRule, PermissionScope, PermissionSettings, PlanStep, Plugin, PluginApp, PluginHook, PluginHookHandler, Project, ProjectChanged, ProviderSlashCommand, ProviderUsage, Question, Refusal, Role, RuleRef, RunOutcome, Schedule, ScheduledRun, ScheduledTask, ScopeRules, Screenshot, Settings, ShellChoices, Skill, StepStatus, SubAgent, TerminalSettings, Theme, TokenSettings, TokenUsage, ToolCall, TurnFile, TurnFileState, TurnTrees, UsageWindow, Weekday };

/**
 * A message as the webview holds it: what the backend saves, plus what a reply adds in the UI while
 * it runs.
 */
export type Message = WireMessage & {
  /** The agent is checking whether the goal is met; not saved. */
  checkingGoal?: boolean;
  /** The agent is retrying a model request that failed or stalled; not saved. */
  retry?: { reason: string; attempt: number; maxAttempts: number } | null;
};

/** A thread as the webview holds it. Set in the UI on a thread from `listThreads`, which carries only its last message and those still streaming, queued, or awaiting approval; `getThread` loads the rest. */
export type Thread = Omit<WireThread, "messages"> & { messages: Message[]; partial?: boolean };

// Typed bridge to the Rust backend. Mirrors apps/server/src/types.rs and the
// commands registered in apps/desktop/src-tauri/src/lib.rs — keep all three in sync.
import { Channel, invoke as tauriInvoke, type InvokeArgs } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { profile } from "~/lib/profile";

const invoke = <T>(cmd: string, args?: InvokeArgs) =>
  profile(`ipc:${cmd}`, () => tauriInvoke<T>(cmd, args));

/** The agents that run an installed CLI. */
export type CliKind = Exclude<AgentKind, "api">;

















/** What to do with the pointer or keyboard. */
export type ComputerAction =
  | { kind: "move"; x: number; y: number }
  | { kind: "click"; x: number; y: number; button?: "left" | "middle" | "right"; double?: boolean }
  | { kind: "type"; text: string }
  | { kind: "key"; key: string; modifiers?: ("ctrl" | "shift" | "alt" | "meta")[] }
  | { kind: "scroll"; amount: number; horizontal?: boolean };








/** How Patchwork keeps itself up to date. */
export interface UpdatesSettings {
  /** Minutes between checks for a new release: 10 to 60, in steps of 10. */
  checkEveryMinutes: number;
}




const isMac = /Mac|iPhone|iPad/.test(navigator.platform);
const isLinux = /Linux/.test(navigator.platform);

/** VS Code's terminal defaults, as `TerminalSettings::default` sets them. */
export function defaultTerminalSettings(): TerminalSettings {
  return {
    shellArgs: null,
    env: {},
    inheritEnv: true,
    cwd: null,
    detectLocale: "auto",
    fontFamily: null,
    fontSize: isMac ? 12 : 14,
    lineHeight: isLinux ? 1.1 : 1,
    letterSpacing: 0,
    fontWeight: "normal",
    fontWeightBold: "bold",
    cursorStyle: "block",
    cursorBlinking: false,
    cursorWidth: 1,
    scrollback: 1000,
    copyOnSelection: false,
    rightClickBehavior: isMac ? "selectWord" : isLinux ? "default" : "copyPaste",
    altClickMovesCursor: true,
    macOptionIsMeta: false,
    drawBoldTextInBrightColors: true,
    minimumContrastRatio: 4.5,
    smoothScrolling: false,
    mouseWheelScrollSensitivity: 1,
    fastScrollSensitivity: 5,
  };
}

/** A shell the terminal started. */
export interface StartedTerminal {
  id: number;
  /** The shell's file name, such as "zsh". */
  name: string;
  cwd: string;
}








/** A saved provider, which never carries its key back to the UI. */
export interface ApiProviderView extends ApiProvider {
  hasKey: boolean;
}


export interface McpTool {
  name: string;
  description: string;
}

export interface McpServerView extends McpServer {
  name: string;
  status: "off" | "starting" | "ready" | "error";
  tools: McpTool[];
  error: string | null;
  /** Equal for the same server wherever it is configured; a CLI server's `key`. */
  identity: string;
}

/** A server Claude Code or Codex runs, read from its own config. Mirrors `CliServer` in
 * apps/server/src/provider/mcp_clis.rs; environment and header values stay out. */
export interface CliMcpServer {
  /** Names it for a refresh; equal for the same server wherever it is configured. */
  key: string;
  name: string;
  scope: "user" | "project" | "local";
  /** The Patchwork project a project or local server belongs to. */
  project: string | null;
  file: string;
  command: string | null;
  args: string[];
  url: string | null;
  /** Names of its environment variables, or of its headers for a URL. */
  env: string[];
  /** `unapproved`: a project's server the CLI has not approved, so it is not started. */
  status: "off" | "unapproved" | "listing" | "ready" | "signin" | "error";
  /** Its turns in Patchwork use Patchwork's copy of it. */
  relayed: boolean;
  tools: McpTool[];
  error: string | null;
}

export interface CliMcpGroup {
  agent: AgentKind;
  file: string;
  servers: CliMcpServer[];
}

/** What happens to a provider's stored key when it is saved. */
export type KeyChange = { action: "keep" } | { action: "set"; key: string } | { action: "clear" };










export interface SkillsView {
  /** The skills folder in use. */
  dir: string;
  skills: Skill[];
}

/** A custom agent in Patchwork's agents folder; "&name" in a message summons it. */
export interface CustomAgent {
  /** The file's name without `.md`. */
  name: string;
  /** The frontmatter's `name`, when it differs from the file's. */
  title: string | null;
  description: string | null;
  /** The model the chat switches to when the agent is summoned, if the chat's CLI has it. */
  model: string | null;
  /** Summoned, it stays the chat's agent until another replaces it. */
  primary: boolean;
  path: string;
}

export interface AgentsView {
  /** The agents folder. */
  dir: string;
  agents: CustomAgent[];
}
















/** How a file a reply produced is shown: `image` as a thumbnail, `text` in the diff list, `other` as a card. */
export type FileMediaKind = "image" | "text" | "other";









export const DEFAULT_SETTINGS: Settings = {
  onboarded: false,
  theme: "dark",
  agents: [],
  integrations: { github: false, terminal: false, figma: false, browser: false },
  projects: [],
  activeProjectId: null,
  agent: null,
  importLogDisabled: false,
  agentPaths: {},
  shell: null,
  skillsDir: null,
  disabledSkills: [],
  visionModel: null,
  tokens: {
    showUsage: true,
    warnAbove: 150_000,
    askBeforeResume: true,
    claudeCompactWindow: 200_000,
    compactWindowDefaulted: true,
    defaultModels: {},
    generateTitles: true,
  },
  notifications: { goalComplete: true, taskComplete: true },
  updates: { checkEveryMinutes: 10 },
  logo: null,
  logging: { app: "info", window: "info", agents: "info", imports: "info" },
  terminal: defaultTerminalSettings(),
  permissions: {
    everywhere: { defaults: {}, rules: [] },
    chats: { defaults: {}, rules: [] },
    projects: {},
    seeded: false,
  },
  confirmAnswers: false,
  pluginsDir: null,
  disabledPlugins: [],
  computerUse: false,
};

/** Agent used for new tasks: the first connected one, else Claude Code. */
export function defaultAgentOf(settings: Settings): AgentKind {
  return settings.agents[0] ?? "claude";
}

export const AGENT_LABELS: Record<AgentKind, string> = {
  claude: "Claude Code",
  codex: "Codex",
  antigravity: "Antigravity CLI",
  opencode: "OpenCode CLI",
  api: "API",
};

export const SOURCE_LABELS: Record<ImportSource, string> = {
  claude: AGENT_LABELS.claude,
  codex: AGENT_LABELS.codex,
  antigravity: AGENT_LABELS.antigravity,
  t3: "T3 Code",
  opencode: "OpenCode",
};

/** Import sources to offer: each connected agent's own files, plus another app and OpenCode. */
export function importSourcesFor(agents: AgentKind[]): ImportSource[] {
  const cli = agents.filter((a): a is CliKind => a !== "opencode" && a !== "api");
  return [...cli, "t3", "opencode"];
}

const agentStatusCache: Partial<Record<AgentKind, AgentStatus>> = {};

/** Each API provider's preset, name, and logo, by id, as last listed; an API model shows them. */
const apiProviders: Record<string, { preset: string; name: string; logo: string | null }> = {};
const rememberPresets = <T extends ApiProvider>(providers: T[]) => {
  for (const p of providers) apiProviders[p.id] = { preset: p.preset, name: p.name, logo: p.logo };
  return providers;
};

/** The provider of a `provider/model` id, as last listed. */
export function apiProviderOf(model: string | null | undefined) {
  return model ? apiProviders[model.split("/")[0]] : undefined;
}

const API_PROVIDERS_CHANGED = "patchwork-api-providers-changed";
const apiProvidersChanged = () => {
  delete agentStatusCache.api;
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
const pendingDetect: Partial<Record<AgentKind, Promise<AgentStatus>>> = {};

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  /** The rules Settings → Permissions starts with, which Restore defaults puts back. */
  defaultPermissionRules: () => invoke<PermissionRule[]>("default_permission_rules"),
  detectAgent: async (kind: AgentKind, forceRefresh = false): Promise<AgentStatus> => {
    if (!forceRefresh && agentStatusCache[kind]) {
      return agentStatusCache[kind]!;
    }
    if (!pendingDetect[kind]) {
      pendingDetect[kind] = invoke<AgentStatus>("detect_agent", { kind })
        .then((status) => {
          agentStatusCache[kind] = status;
          delete pendingDetect[kind];
          return status;
        })
        .catch((err) => {
          delete pendingDetect[kind];
          throw err;
        });
    }
    return pendingDetect[kind]!;
  },
  /** Models for an agent, starting with "Default" (the CLI's own choice). */
  listModels: async (kind: AgentKind) => {
    // API models show their provider's logo, so the presets are read alongside.
    if (kind === "api") await api.listApiProviders().catch(() => []);
    return invoke<ModelOption[]>("list_models", { kind });
  },
  listApiProviders: () =>
    invoke<ApiProviderView[]>("list_api_providers").then(rememberPresets),
  /** Adds a provider (`id` empty) or replaces one; the key is stored apart from the provider. */
  saveApiProvider: (provider: ApiProvider, key: KeyChange) =>
    invoke<ApiProviderView>("save_api_provider", { provider, key }).then((p) => {
      rememberPresets([p]);
      apiProvidersChanged();
      return p;
    }),
  removeApiProvider: (id: string) =>
    invoke<void>("remove_api_provider", { id }).then(apiProvidersChanged),
  listMcpServers: () => invoke<McpServerView[]>("list_mcp_servers"),
  /** Adds or replaces a server; `previous` is its old name when renamed. */
  saveMcpServer: (name: string, server: McpServer, previous: string | null = null) =>
    invoke<void>("save_mcp_server", { name, previous, server }),
  removeMcpServer: (name: string) => invoke<void>("remove_mcp_server", { name }),
  restartMcpServer: (name: string) => invoke<void>("restart_mcp_server", { name }),
  mcpConfigFile: () => invoke<string>("mcp_config_file"),
  /** How many MCP tools an API turn may offer. */
  mcpToolBudget: () => invoke<number>("mcp_tool_budget"),
  listCliMcpServers: () => invoke<CliMcpGroup[]>("list_cli_mcp_servers"),
  /** Reads the server's tools again. */
  refreshCliMcpServer: (key: string) => invoke<void>("refresh_cli_mcp_server", { key }),
  /** Adds a CLI's server to Patchwork's own, for API tasks. */
  enableCliMcpServer: (key: string) => invoke<void>("enable_cli_mcp_server", { key }),
  openMcpConfig: (path: string) => invoke<void>("open_mcp_config", { path }),
  /** The endpoint's own model list, which also checks the URL and key. A key typed but not saved
   * goes in `key`; otherwise the saved provider's key is used. */
  fetchApiModels: (query: {
    baseUrl: string;
    headers: { name: string; value: string }[];
    key: string | null;
    keyEnv: string | null;
    providerId: string | null;
  }) => invoke<ApiModel[]>("fetch_api_models", { query }),
  /** Installed login shells, as paths from /etc/shells, and the default shell's path. */
  listShells: () => invoke<ShellChoices>("list_shells"),
  listSkills: () => invoke<SkillsView>("list_skills"),
  /** The plugin bundles in the plugins folder, with their skills and MCP servers. */
  listPlugins: () => invoke<Plugin[]>("list_plugins"),
  openPluginsFolder: () => invoke<void>("open_plugins_folder"),
  /** The scheduled tasks, as saved. */
  listScheduledTasks: () => invoke<ScheduledTask[]>("list_scheduled_tasks"),
  /** Adds or replaces a task, and returns it with its next run worked out. */
  saveScheduledTask: (task: ScheduledTask) => invoke<ScheduledTask>("save_scheduled_task", { task }),
  removeScheduledTask: (id: string) => invoke<void>("remove_scheduled_task", { id }),
  /** Starts a task now, whatever its schedule says. */
  runScheduledTask: (id: string) => invoke<Thread>("run_scheduled_task", { id }),
  /** What the installed Codex CLI offers: its version, flags, and models. */
  codexInfo: () => invoke<CodexInfo>("codex_info"),
  /** The output of `codex doctor`, `codex mcp list`, or `codex plugin list`. */
  codexCommand: (kind: "doctor" | "mcp" | "plugins") => invoke<string>("codex_command", { kind }),
  /** The screen as a PNG data URL; refuses unless computer use is on. */
  computerScreenshot: () => invoke<Screenshot>("computer_screenshot"),
  computerAction: (action: ComputerAction) => invoke<void>("computer_action", { action }),
  openSkillsFolder: () => invoke<void>("open_skills_folder"),
  /** The skills now, and on `onChange` after each change to the folder until `unwatchSkills`. */
  watchSkills: (onChange: (view: SkillsView) => void) => {
    const channel = new Channel<SkillsView>();
    channel.onmessage = onChange;
    return invoke<SkillsView>("watch_skills", { onChange: channel });
  },
  unwatchSkills: () => invoke<void>("unwatch_skills"),
  openSkill: (path: string) => invoke<void>("open_skill", { path }),
  /** The agents now, and on `onChange` after each change to them, until another watch. */
  watchAgents: (onChange: (view: AgentsView) => void) => {
    const channel = new Channel<AgentsView>();
    channel.onmessage = onChange;
    return invoke<AgentsView>("watch_agents", { onChange: channel });
  },
  openAgent: (path: string) => invoke<void>("open_agent", { path }),
  /** The agents a chat in the project can summon, with the project's own. */
  listAgents: (projectId: string | null) => invoke<CustomAgent[]>("list_agents", { projectId }),
  createAgent: (name: string) => invoke<void>("create_agent", { name }),
  setThreadRole: (id: string, role: string | null) =>
    invoke<void>("set_thread_role", { id, role }),
  openLogsFolder: () => invoke<void>("open_logs_folder"),
  exportLogs: (path: string) => invoke<void>("export_logs", { path }),
  logChannels: () => invoke<string[]>("log_channels"),
  readLog: (channel: string, offset: number) => invoke<LogChunk>("read_log", { channel, offset }),
  logRenderer: (level: LogLevel, message: string) =>
    invoke<void>("log_renderer", { level, message }),
  terminalSpawn: (cwd: string | null, cols: number, rows: number) =>
    invoke<StartedTerminal>("terminal_spawn", { cwd, cols, rows }),
  terminalWrite: (id: number, data: string) => invoke<void>("terminal_write", { id, data }),
  terminalResize: (id: number, cols: number, rows: number) =>
    invoke<void>("terminal_resize", { id, cols, rows }),
  terminalKill: (id: number) => invoke<void>("terminal_kill", { id }),
  /** Subscription usage limits. Cached by the backend; `refresh` asks for new
   * numbers, which it still throttles to avoid provider rate limits. */
  getUsage: (kind: AgentKind, refresh = false) =>
    invoke<ProviderUsage>("get_usage", { kind, refresh }),
  /** Counts conversations `kind` saved on disk that are not threads yet. */
  scanImports: (kind: ImportSource) => invoke<ImportScan>("scan_imports", { kind }),
  /** Imports those conversations as threads; returns how many were added. */
  importThreads: (kinds: ImportSource[]) => invoke<number>("import_threads", { kinds }),

  /** Registers an existing folder; also makes it the active project. */
  addProject: (path: string) => invoke<Project>("add_project", { path }),
  /** Creates `parent/name`, runs `git init`, registers it as the active project. */
  createProject: (parent: string, name: string) =>
    invoke<Project>("create_project", { parent, name }),
  renameProject: (id: string, name: string) => invoke<void>("rename_project", { id, name }),
  relocateProject: (id: string, path: string) => invoke<void>("relocate_project", { id, path }),
  isFolder: (path: string) => invoke<boolean>("is_folder", { path }),
  removeProject: (id: string) => invoke<void>("remove_project", { id }),

  /** All threads (project tasks and project-less chats), in sidebar order. */
  /** Every thread, each with only the messages the task list reads (see `Thread.partial`). */
  listThreads: () => invoke<Thread[]>("list_threads"),
  /** One thread with all its messages. */
  getThread: (id: string) => invoke<Thread>("get_thread", { id }),
  /** Puts threads in a folder (null: the top level) in the given order. */
  reorderThreads: (ids: string[], folderId: string | null) =>
    invoke<void>("reorder_threads", { ids, folderId }),
  listFolders: () => invoke<Folder[]>("list_folders"),
  /** Files a mention can refer to, relative to the project root. */
  listFiles: (projectId: string) => invoke<string[]>("list_files", { projectId }),
  /** Whether the Antigravity CLI is installed in Patchwork's CLI folder. */
  antigravityInstalled: () => invoke<boolean>("antigravity_installed"),
  /** Downloads and installs the Antigravity CLI into Patchwork's CLI folder. */
  installAntigravity: () => invoke<void>("install_antigravity"),
  /** Deletes a chat's messages and agent session; the chat stays, empty. */
  clearThread: (id: string) => invoke<void>("clear_thread", { id }),
  /** The user messages of a thread whose turn can be undone: the project was snapshotted before it. */
  checkpoints: (threadId: string) => invoke<string[]>("checkpoints", { threadId }),
  /** Puts the project's files back as they were before the message's turn, and removes that message
   * and the ones after it. Returns the thread as it is now. */
  restoreCheckpoint: (threadId: string, messageId: string) =>
    invoke<Thread>("restore_checkpoint", { threadId, messageId }),
  forkThread: (threadId: string, messageId: string) =>
    invoke<Thread>("fork_thread", { threadId, messageId }),
  /** The thread that was open when the app last closed. */
  getLastThread: () => invoke<string | null>("get_last_thread"),
  setLastThread: (id: string) => invoke<void>("set_last_thread", { id }),
  /** Ids of the projects and folders collapsed in the sidebar. */
  getCollapsed: () => invoke<string[]>("get_collapsed"),
  setCollapsed: (ids: string[]) => invoke<void>("set_collapsed", { ids }),
  createFolder: (projectId: string, name: string) =>
    invoke<Folder>("create_folder", { projectId, name }),
  renameFolder: (id: string, name: string) => invoke<void>("rename_folder", { id, name }),
  /** Its tasks move to the project's top level. */
  removeFolder: (id: string) => invoke<void>("remove_folder", { id }),
  createThread: (
    projectId: string | null,
    agent: AgentKind,
    model: string | null,
    accessMode: AccessMode,
    effort: string | null,
  ) =>
    invoke<Thread>("create_thread", {
      request: { projectId, agent, model, accessMode, effort },
    }),
  deleteThread: (id: string) => invoke<void>("delete_thread", { id }),
  setThreadArchived: (id: string, archived: boolean) =>
    invoke<void>("set_thread_archived", { id, archived }),
  renameThread: (id: string, title: string) => invoke<void>("rename_thread", { id, title }),
  /** Sets agent + model; the agent can only change before the first message. */
  setThreadModel: (id: string, agent: AgentKind, model: string | null) =>
    invoke<void>("set_thread_model", { id, agent, model }),
  /** Takes effect from the thread's next turn. */
  setThreadAccessMode: (id: string, mode: AccessMode) =>
    invoke<void>("set_thread_access_mode", { id, mode }),
  /** Writes `text` to `path`, a file the user picked in a save dialog. */
  saveTextFile: (path: string, text: string) => invoke<void>("save_text_file", { path, text }),
  setThreadPlanMode: (id: string, on: boolean) =>
    invoke<void>("set_thread_plan_mode", { id, on }),
  setThreadEffort: (id: string, effort: string | null) =>
    invoke<void>("set_thread_effort", { id, effort }),
  /** Returns the stored user message; assistant output streams via onAgentEvent. */
  sendMessage: (threadId: string, text: string, attachments: Attachment[], queue: boolean) =>
    invoke<Message>("send_message", { threadId, text, attachments, queue }),
  /** Classifies a dropped path as a file, folder, or image. */
  attachmentForPath: (path: string) => invoke<Attachment>("attachment_for_path", { path }),
  /** Saves a pasted image; `data` is the file's bytes, base64-encoded. */
  savePastedImage: (mimeType: string, data: string, name: string) =>
    invoke<Attachment>("save_pasted_image", { mimeType, data, name }),
  /** Saves the system clipboard's image to attach, or null when it holds none. */
  pasteClipboardImage: () => invoke<Attachment | null>("paste_clipboard_image"),
  /** The paths of files and folders copied in a file manager, or none. */
  clipboardFiles: () => invoke<string[]>("clipboard_files"),
  /** The clipboard copy's VS Code editor metadata (JSON), or null when it has none. */
  clipboardEditorData: () => invoke<string | null>("clipboard_editor_data"),
  /** Copies text through the system clipboard, for when the webview refuses to. */
  copyText: (text: string) => invoke<void>("copy_text", { text }),
  /** Saves a long pasted text as a file to attach. */
  savePastedText: (text: string) => invoke<Attachment>("save_pasted_text", { text }),
  /** Lets the webview show an attached image as a thumbnail. */
  allowPreview: (path: string) => invoke<void>("allow_preview", { path }),
  cancelTurn: (threadId: string) => invoke<void>("cancel_turn", { threadId }),
  /** Stops one running subagent (Claude only). */
  stopSubagent: (threadId: string, toolId: string) =>
    invoke<void>("stop_subagent", { threadId, toolId }),
  /** Takes back a message queued behind the running turn, so it can be edited. */
  removeQueued: (threadId: string, messageId: string) =>
    invoke<void>("remove_queued", { threadId, messageId }),
  editQueued: (threadId: string, messageId: string, text: string, attachments: Attachment[]) =>
    invoke<void>("edit_queued", { threadId, messageId, text, attachments }),
  resumeQueued: (threadId: string) => invoke<void>("resume_queued", { threadId }),
  steerQueued: (threadId: string, messageId: string) =>
    invoke<void>("steer_queued", { threadId, messageId }),
  /** Clears the goal, in the agent's session too. Refused while a turn runs. */
  stopGoal: (threadId: string) => invoke<void>("stop_goal", { threadId }),
  respondApproval: (
    threadId: string,
    approvalId: string,
    allow: boolean,
    answers?: Record<string, string[]>,
    rule?: RuleRef,
  ) => invoke<void>("respond_approval", { threadId, approvalId, allow, answers, rule }),

  readFile: (projectId: string, path: string) => invoke<string>("read_file", { projectId, path }),
  /** Which of `paths` no longer exist in the project. */
  missingFiles: (projectId: string, paths: string[]) =>
    invoke<string[]>("missing_files", { projectId, paths }),
  /** What each of a reply's files is shown as: a thumbnail, a card, or a line in the diff list. */
  fileMedia: (projectId: string, paths: string[]) =>
    invoke<FileMedia[]>("file_media", { projectId, paths }),
  /** Opens a file inside the project in the default app. */
  openFile: (projectId: string, path: string) => invoke<void>("open_file", { projectId, path }),
  turnFiles: (projectId: string, turn: TurnTrees, paths: string[]) =>
    invoke<TurnFile[]>("turn_files", { projectId, turn, paths }),
  turnFileDiff: (projectId: string, turn: TurnTrees, path: string) =>
    invoke<string>("turn_file_diff", { projectId, turn, path }),
  gitBranch: (projectId: string) => invoke<string | null>("git_branch", { projectId }),
  gitChanges: (projectId: string) => invoke<FileChange[]>("git_changes", { projectId }),
  /** Unified diff text for one file (empty string if unchanged). */
  gitFileDiff: (projectId: string, path: string) =>
    invoke<string>("git_file_diff", { projectId, path }),
  /** "Apply changes": stages the file (git add). */
  acceptFile: (projectId: string, path: string) => invoke<void>("accept_file", { projectId, path }),
  /** "Reject": restores the file to HEAD (deletes it if untracked). */
  revertFile: (projectId: string, path: string) => invoke<void>("revert_file", { projectId, path }),
  openInEditor: (projectId: string, path: string) =>
    invoke<void>("open_in_editor", { projectId, path }),
  /** Shows a file selected in the file manager; a relative path is taken from the project's root. */
  revealFile: (projectId: string | null, path: string) =>
    invoke<void>("reveal_file", { projectId, path }),
  /** Watches the project's files in place of any other; `onProjectChanged` reports changes. */
  watchProject: (projectId: string) => invoke<void>("watch_project", { projectId }),
  /** Opens a web or mail link in the default browser or mail app. */
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  updateStatus: () => invoke<UpdateStatus>("update_status"),
  /** Starts a check; its progress and result arrive through `onUpdateStatus`. */
  checkForUpdates: () => invoke<void>("check_for_updates"),
  /** Restarts into the installed update; without `force`, refuses while agents are replying. */
  restartToUpdate: (force: boolean) => invoke<RestartResult>("restart_to_update", { force }),
};

export function onAgentEvent(cb: (e: AgentEventEnvelope) => void): Promise<UnlistenFn> {
  return listen<AgentEventEnvelope>("agent-event", (e) => cb(e.payload));
}


export function onProjectChanged(cb: (e: ProjectChanged) => void): Promise<UnlistenFn> {
  return listen<ProjectChanged>("project-changed", (e) => cb(e.payload));
}

export function onTerminalOutput(cb: (e: { id: number; data: string }) => void): Promise<UnlistenFn> {
  return listen<{ id: number; data: string }>("terminal-output", (e) => cb(e.payload));
}

export function onTerminalExit(
  cb: (e: { id: number; code: number | null }) => void,
): Promise<UnlistenFn> {
  return listen<{ id: number; code: number | null }>("terminal-exit", (e) => cb(e.payload));
}

export function onUpdateStatus(cb: (status: UpdateStatus) => void): Promise<UnlistenFn> {
  return listen<UpdateStatus>("update-status", (e) => cb(e.payload));
}

/** Where updating Patchwork stands. Mirrors `UpdateState` in apps/desktop/src-tauri/src/updates.rs. */
export type UpdateState =
  | { status: "idle" }
  | { status: "checking" }
  | { status: "upToDate" }
  /** A newer release this copy cannot install itself. */
  | { status: "available"; version: string }
  | { status: "downloading"; version: string; downloaded: number; total: number | null }
  /** Installed; it runs once Patchwork restarts. */
  | { status: "ready"; version: string }
  | { status: "failed"; message: string };

export interface UpdateStatus {
  currentVersion: string;
  /** Whether this copy can install updates itself; a dev or source build cannot. */
  canInstall: boolean;
  state: UpdateState;
  /** The notes of the newer release the last check found, as Markdown. */
  notes: string | null;
}

export type RestartResult =
  | { result: "busy"; running: number }
  | { result: "nothingReady" }
  | { result: "restarting" };
