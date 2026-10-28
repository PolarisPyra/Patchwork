// Typed bridge to the Rust backend. Mirrors apps/server/src/types.rs and the
// commands registered in apps/desktop/src-tauri/src/lib.rs — keep all three in sync.
import { Channel, invoke as tauriInvoke, type InvokeArgs } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { profile } from "~/lib/profile";

const invoke = <T>(cmd: string, args?: InvokeArgs) =>
  profile(`ipc:${cmd}`, () => tauriInvoke<T>(cmd, args));

export type AgentKind = "claude" | "codex" | "antigravity" | "opencode" | "api";
/** The agents that run an installed CLI. */
export type CliKind = Exclude<AgentKind, "api">;
export type Theme = "light" | "dark" | "system";
export type Role = "user" | "assistant";
export type MessageStatus = "streaming" | "complete" | "error" | "cancelled";
export type StepStatus = "pending" | "running" | "done" | "error";
export type AttachmentKind = "image" | "file" | "folder";

export interface Integrations {
  github: boolean;
  terminal: boolean;
  figma: boolean;
  browser: boolean;
}

export interface Project {
  id: string;
  name: string;
  path: string;
}

export interface Settings {
  onboarded: boolean;
  theme: Theme;
  /** Connected agents; the first is the default for new tasks. */
  agents: AgentKind[];
  /** Agent CLIs the user located by hand, used instead of searching for them. */
  agentPaths: Partial<Record<AgentKind, string>>;
  integrations: Integrations;
  projects: Project[];
  activeProjectId: string | null;
  logging: LoggingSettings;
  terminal: TerminalSettings;
  tokens: TokenSettings;
  /** Shell that agents run commands in; null follows the SHELL environment variable. */
  shell: string | null;
  /** Folder of the skills every agent loads; null uses `skills` in the config folder. */
  skillsDir: string | null;
  /** Skills switched off in Settings, by name. */
  disabledSkills: string[];
  notifications: NotificationSettings;
  updates: UpdatesSettings;
  /** Id of the app icon color way in `assets/logos/manifest.json`; null uses the manifest default. */
  logo: string | null;
  permissions: PermissionSettings;
  /** Answering an agent's question takes a second click on the picked answer. */
  confirmAnswers: boolean;
  /** Folder of plugin bundles; null uses `plugins` in the config folder. */
  pluginsDir: string | null;
  /** Plugins switched off in Settings, by folder name. */
  disabledPlugins: string[];
  /** Whether the agent may look at the screen and drive the pointer and keyboard. */
  computerUse: boolean;
}

/** A scheduled task: a prompt that starts a task at a fixed time. Mirrors `ScheduledTask`. */
export interface ScheduledTask {
  id: string;
  name: string;
  prompt: string;
  schedule: Schedule;
  /** The project to run in; null starts a chat in a scratch folder. */
  projectId: string | null;
  agent: AgentKind;
  model: string | null;
  accessMode: AccessMode;
  enabled: boolean;
  lastRunAt: number | null;
  nextRunAt: number | null;
  createdAt: number;
}

export type Weekday = "MO" | "TU" | "WE" | "TH" | "FR" | "SA" | "SU";

/** How often a scheduled task runs, in the shapes Codex's tasks use. */
export type Schedule =
  | { type: "hourly"; intervalHours: number }
  | { type: "daily"; time: string }
  | { type: "weekdays"; time: string }
  | { type: "weekly"; days: Weekday[]; time: string };

/** An MCP server a plugin offers. */
export interface PluginMcpServer {
  name: string;
  command: string;
  args: string[];
}

/** A plugin bundle: a folder with a `plugin.json`, its skills, and the MCP servers it adds. */
export interface Plugin {
  /** The folder name, and what `disabledPlugins` records. */
  id: string;
  name: string;
  version: string | null;
  description: string | null;
  path: string;
  skills: Skill[];
  mcpServers: PluginMcpServer[];
  enabled: boolean;
  bundled: boolean;
}

/** One entry in the memory tree: a place an agent takes instructions from. */
export interface MemoryNode {
  name: string;
  /** The file to read, or null for a group heading. */
  path: string | null;
  kind: MemoryKind;
  /** A skill's description, or why a file is listed and missing. */
  note: string | null;
  size: number | null;
  children: MemoryNode[];
}

export type MemoryKind = "file" | "folder" | "skill";

/** A feature flag of the installed Codex CLI. */
export interface CodexFeature {
  name: string;
  /** `stable`, `beta`, `under development`, `deprecated`, or `removed`. */
  stage: string;
  enabled: boolean;
}

/** A model in the installed Codex CLI's catalog. */
export interface CodexModel {
  slug: string;
  displayName: string;
  contextWindow: number | null;
  efforts: string[];
  speedTiers: string[];
  multiAgent: string | null;
}

/** What the installed Codex CLI offers. */
export interface CodexInfo {
  path: string;
  version: string;
  features: CodexFeature[];
  models: CodexModel[];
}

/** The screen as a PNG data URL, and the size of the screen it came from. */
export interface Screenshot {
  dataUrl: string;
  width: number;
  height: number;
  screenWidth: number;
  screenHeight: number;
}

/** What to do with the pointer or keyboard. */
export type ComputerAction =
  | { kind: "move"; x: number; y: number }
  | { kind: "click"; x: number; y: number; button?: "left" | "middle" | "right"; double?: boolean }
  | { kind: "type"; text: string }
  | { kind: "key"; key: string; modifiers?: ("ctrl" | "shift" | "alt" | "meta")[] }
  | { kind: "scroll"; amount: number; horizontal?: boolean };

/** What agents may do without asking. Mirrors `PermissionSettings` in apps/server/src/types.rs. */
export interface PermissionSettings {
  everywhere: ScopeRules;
  /** Chats without a project. */
  chats: ScopeRules;
  /** By project id. */
  projects: Record<string, ScopeRules>;
  seeded: boolean;
}

export interface ScopeRules {
  /** What each kind of request gets when no rule matches it; a missing kind inherits. */
  defaults: Partial<Record<PermissionKind, Effect>>;
  rules: PermissionRule[];
}

export interface PermissionRule {
  id: string;
  kind: PermissionKind;
  /** A folder, a command's leading words, a web host, or an MCP server; `*` is anything, and `.`
   * is the thread's own folder. */
  pattern: string;
  effect: Effect;
}

export type PermissionKind = "edit" | "command" | "folder" | "web" | "mcp" | "screen";

export type Effect = "allow" | "ask" | "deny";

/** Where a permission rule applies. */
export type PermissionScope =
  | { type: "everywhere" }
  | { type: "chats" }
  | { type: "project"; projectId: string };

/** The rule that answered an approval: a rule by id, or the scope's default for the kind. */
export interface RuleRef {
  scope: PermissionScope;
  ruleId?: string;
}

/** How Patchwork keeps itself up to date. */
export interface UpdatesSettings {
  /** Minutes between checks for a new release: 10 to 60, in steps of 10. */
  checkEveryMinutes: number;
}

/** How much a log records, as VS Code's log levels; each includes the ones before it. Below
 * Trace no conversation or file content is written. */
export type LogLevel = "off" | "error" | "warning" | "info" | "debug" | "trace";

/** How much each log in the launch's log folder records. */
export interface LoggingSettings {
  /** `main.log`: Patchwork's own messages. */
  app: LogLevel;
  /** `window/renderer.log`: the window's errors and warnings. */
  window: LogLevel;
  /** `agents/<agent>.log`: each agent's processes and their traffic. */
  agents: LogLevel;
  /** `imports.log`: each scan and import of saved conversations. */
  imports: LogLevel;
}

/** The integrated terminal, as VS Code's `terminal.integrated.*` settings. Mirrors
 * `TerminalSettings` in apps/server/src/types.rs. */
export interface TerminalSettings {
  /** Null is `-l` on macOS, none elsewhere. */
  shellArgs: string[] | null;
  /** Null removes a variable; values may use `${env:NAME}` and `${workspaceFolder}`. */
  env: Record<string, string | null>;
  inheritEnv: boolean;
  /** Null is the project folder. */
  cwd: string | null;
  detectLocale: "auto" | "off" | "on";
  /** Null is the app's monospace font. */
  fontFamily: string | null;
  fontSize: number;
  lineHeight: number;
  letterSpacing: number;
  fontWeight: string;
  fontWeightBold: string;
  cursorStyle: "block" | "line" | "underline";
  cursorBlinking: boolean;
  cursorWidth: number;
  scrollback: number;
  copyOnSelection: boolean;
  rightClickBehavior: "default" | "copyPaste" | "paste" | "selectWord" | "nothing";
  altClickMovesCursor: boolean;
  macOptionIsMeta: boolean;
  drawBoldTextInBrightColors: boolean;
  minimumContrastRatio: number;
  smoothScrolling: boolean;
  mouseWheelScrollSensitivity: number;
  fastScrollSensitivity: number;
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

/** Part of a log for the Output panel. */
export interface LogChunk {
  text: string;
  /** Where to read from next. */
  offset: number;
  /** The log started over, so what is shown is replaced. */
  reset: boolean;
}

/** Desktop notifications shown while Patchwork is not the focused window. */
export interface NotificationSettings {
  /** When a task's goal is met. */
  goalComplete: boolean;
  /** When an agent finishes replying in a task that is not pursuing a goal. */
  taskComplete: boolean;
}

/** Settings that trade a little convenience for fewer tokens spent. */
export interface TokenSettings {
  /** Shows the token counts under each finished reply. */
  showUsage: boolean;
  /** A context above this is flagged, and a cold task this large is asked about first. */
  warnAbove: number;
  /** Asks before a large task whose prompt cache has expired is resumed. */
  askBeforeResume: boolean;
  /** Compacts an idle Claude task just before its prompt cache expires. */
  autoCompact: boolean;
  /** The context size above which an idle task is compacted. 0 follows `claudeCompactWindow`. */
  autoCompactAbove: number;
  /** The context size at which Claude compacts mid-task. 0 leaves Claude's own. */
  claudeCompactWindow: number;
  /** Set once the 200k default for `claudeCompactWindow` replaced the old default, 0. */
  compactWindowDefaulted: boolean;
  /** Model for new tasks, per agent. A missing agent uses the CLI's own default. */
  defaultModels: Partial<Record<AgentKind, string>>;
  /** Names a new chat or task with a title its provider writes, rather than its first line. */
  generateTitles: boolean;
}

export interface AgentStatus {
  kind: AgentKind;
  installed: boolean;
  version: string | null;
  signedIn: boolean;
  detail: string;
}

/** A model the user can pick for an agent. `id: null` is the CLI's own default. */
export interface ModelOption {
  id: string | null;
  label: string;
  defaultModel?: string | null;
  /** Superseded model; the picker lists it under "Legacy models". */
  isLegacy: boolean;
}

/** An OpenAI-compatible endpoint the user added with their own key. Mirrors `ApiProvider` in
 * apps/server/src/provider/api_config.rs. */
export interface ApiProvider {
  /** The part of a model id before its `/`; empty for a provider not saved yet. */
  id: string;
  name: string;
  /** The preset it was made from, e.g. "deepseek", or "custom". */
  preset: string;
  baseUrl: string;
  enabled: boolean;
  models: ApiModel[];
  headers: { name: string; value: string }[];
  /** Reads the key from this environment variable instead of a stored key. */
  keyEnv: string | null;
  /** Where the saved key is: the system credential store, or Patchwork's keys file. */
  keyStore: "keychain" | "file" | null;
  /** A logo chosen for it, as an image `data:` URL; replaces the preset's. */
  logo: string | null;
}

export interface ApiModel {
  id: string;
  name: string | null;
}

/** A saved provider, which never carries its key back to the UI. */
export interface ApiProviderView extends ApiProvider {
  hasKey: boolean;
}

/** An MCP server API tasks can use. Mirrors `McpServer` in apps/server/src/provider/mcp.rs,
 * which keeps the `mcpServers` shape other MCP clients use. */
export interface McpServer {
  command?: string;
  args?: string[];
  env?: Record<string, string>;
  url?: string;
  headers?: Record<string, string>;
  disabled?: boolean;
  /** Tools left out of API turns, by name. */
  disabledTools?: string[];
  /** Claude Code and Codex turns use Patchwork's copy instead of starting their own. */
  share?: boolean;
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


/** Shells from /etc/shells, and the login shell that "System default" resolves to. */
export interface ShellChoices {
  shells: string[];
  default: string | null;
}

export interface ToolCall {
  id: string;
  name: string;
  summary: string;
  status: StepStatus;
  subagent: SubAgent | null;
  /** The subagent call (its id) this call ran inside, if any. */
  parent: string | null;
  /** When the call started and ended (ms since epoch); null if unknown. */
  startedAt: number | null;
  endedAt: number | null;
  /** What the call returned, cut to its first 16 KB. */
  output?: string;
  /** The thread holding the subagent's own transcript, for a call that started one. */
  childThreadId?: string;
}

/** One step of a reply, in the order it happened. */
export type Part =
  | { type: "text"; text: string }
  /** The agent's reasoning; `endedAt` is null while it still streams. */
  | { type: "thinking"; text: string; startedAt: number; endedAt: number | null }
  /** A tool call, by its id in the message's `tools`. */
  | { type: "tool"; id: string }
  /** A plan the agent proposed in plan mode, as Markdown. */
  | { type: "plan"; text: string }
  /** An approval or question, by its id in the message's `approvals`, where it was asked. */
  | { type: "approval"; id: string };

/** A shell command the agent started in the background, still running. */
export interface BackgroundTask {
  /** The tool call that started it; stops it. */
  toolId: string;
  description: string;
}

/** A subagent an agent started, e.g. with Claude Code's Task tool. */
export interface SubAgent {
  kind: string;
  description: string;
  /** null means the subagent inherits the parent's model. */
  model: string | null;
  /** The instructions it was given. */
  prompt?: string | null;
}

export interface PlanStep {
  text: string;
  status: StepStatus;
}

export interface ProviderSlashCommand {
  name: string;
  description: string | null;
}

/** A skill in Patchwork's skills folder; "$name" in a message invokes it. */
export interface Skill {
  name: string;
  description: string | null;
  /** Its `SKILL.md`. */
  path: string;
  /** Switched on in Settings; agents only load enabled skills. */
  enabled: boolean;
}

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

/** How much an agent may do without asking. */
export type AccessMode = "supervised" | "auto-accept-edits" | "auto" | "full-access";

export type ApprovalStatus = "pending" | "allowed" | "denied";

/** An action the agent asked permission for during a turn. */
export interface Approval {
  id: string;
  /** e.g. "Run command", "Edit files". */
  title: string;
  detail: string;
  status: ApprovalStatus;
  /** What the agent asks the user, when it asks rather than wanting permission. */
  questions?: Question[];
  /** The user's answers, by question id. */
  answers?: Record<string, string[]>;
  /** What kind of request it is, when a permission rule can answer it. */
  kind?: PermissionKind;
  /** The patterns "Always allow" saves; none when a rule could not safely cover it. */
  suggestions?: string[];
  /** The rule that answered it, or that the user saved when answering it. */
  rule?: RuleRef;
  /** Answered by a rule, without asking. */
  auto?: boolean;
}

/** A question the agent asks the user, with the answers it offers. */
export interface Question {
  id: string;
  /** A short label, such as "Library". */
  header: string;
  question: string;
  options: { label: string; description: string }[];
  multiSelect: boolean;
}

export interface Message {
  id: string;
  role: Role;
  text: string;
  createdAt: number; // epoch ms
  status: MessageStatus;
  tools: ToolCall[];
  plan: PlanStep[];
  files: string[]; // project-relative paths edited during this message
  error: string | null;
  approvals: Approval[];
  attachments: Attachment[];
  /** Sent while a turn was running, steering it. */
  steer: boolean;
  /** Sent while a turn was running and still waiting for it to end; then it starts the next turn. */
  queued: boolean;
  /** Background shell commands still running after the reply ended. */
  background: BackgroundTask[];
  /** Cut short by a steering message; the agent answered it in the next reply. */
  cutOff: boolean;
  /** The reply ended and the agent is checking it against the goal; not saved. */
  checkingGoal?: boolean;
  /** The agent is retrying a model request that failed or stalled; not saved. */
  retry?: { reason: string; attempt: number; maxAttempts: number } | null;
  /** A safety classifier blocked part of the reply; the agent's stated reason. */
  refusal: Refusal | null;
  /** The reply's text, reasoning, and tool calls in the order they happened; empty for replies
   * saved before this was kept. */
  parts: Part[];
  /** When the reply finished (ms since epoch); null while it runs. */
  finishedAt: number | null;
  /** Tokens the reply used; null for agents that do not report usage. */
  usage: TokenUsage | null;
  /** The project before and after the turn that wrote the reply; null outside git projects and
   * for older replies. */
  turn?: TurnTrees | null;
}

/** Snapshots of a project's files, as git tree ids. */
export interface TurnTrees {
  before: string;
  after: string;
}

/** Where a file a turn changed stands now. */
export type TurnFileState = "uncommitted" | "committed" | "reverted" | "deleted" | "unchanged";

/** A file a turn changed: lines the turn added and removed, and where the file stands now. */
export interface TurnFile {
  path: string;
  additions: number;
  deletions: number;
  state: TurnFileState;
}

/** Why a safety classifier blocked a reply, as the agent reports it. */
export interface Refusal {
  /** The policy area, such as `cyber`; null when the agent names none. */
  category: string | null;
  explanation: string;
}

/** Tokens a reply used, summed over the model requests it made. */
export interface TokenUsage {
  /** Input that was neither read from nor written to the prompt cache. */
  input: number;
  cacheRead: number;
  cacheWrite: number;
  output: number;
  /** Size of the prompt of the agent's last request: the context it is working in. */
  context: number;
}

/** Something dropped into the composer, sent to the agent with the message. */
export interface Attachment {
  kind: AttachmentKind;
  /** Absolute path on disk. */
  path: string;
  /** Last path component, for display. */
  name: string;
}

export type GoalStatus =
  "active" | "paused" | "blocked" | "usageLimited" | "budgetLimited" | "complete";

/** A `/goal` the agent keeps working toward until it is met. */
export interface Goal {
  objective: string;
  status: GoalStatus;
  /** When the goal was set (ms since epoch). */
  startedAt: number;
  /** Evaluator checks so far (Claude). */
  checks: number | null;
  /** Tokens spent and the budget, and time spent, as the agent reports them (Codex). */
  tokensUsed: number | null;
  tokenBudget: number | null;
  timeUsedSeconds: number | null;
}

export interface Thread {
  id: string;
  /** null for a chat without a project (runs in a private scratch folder). */
  projectId: string | null;
  title: string;
  agent: AgentKind;
  model: string | null;
  accessMode: AccessMode;
  /** Reasoning effort passed to the agent, e.g. "high"; null leaves the CLI's default. */
  effort: string | null;
  sessionId: string | null;
  createdAt: number;
  messages: Message[];
  goal: Goal | null;
  /** The folder this task is in; null for the project's top level. */
  folderId: string | null;
  /** When the thread was archived; null while it is in the task list. */
  archivedAt: number | null;
  /** Set on a subagent's thread: the thread and call that started it. Such a thread is
   * read-only and not listed in the sidebar. */
  parent: { threadId: string; toolId: string } | null;
  /** Turns run in plan mode: the agent plans and proposes, without changing files. */
  planMode: boolean;
  slashCommands: ProviderSlashCommand[];
  /** An imported chat that has not been sent yet: its agent can still be picked, and its
   * transcript goes to the agent with the first message. */
  contextPending: boolean;
  /** The primary custom agent the chat is in the role of. */
  role: string | null;
  /** Set in the UI on a thread from `listThreads`, which carries only its last message and those
   * still streaming, queued, or awaiting approval; `getThread` loads the rest. */
  partial?: boolean;
}

/** A folder inside a project; its tasks are the threads whose `folderId` is its id. */
export interface Folder {
  id: string;
  projectId: string;
  name: string;
}

export interface FileChange {
  path: string;
  status: "modified" | "added" | "deleted" | "untracked" | "renamed";
  additions: number;
  deletions: number;
  staged: boolean;
}

/** One rate-limit window, e.g. the 5-hour session limit. */
export interface UsageWindow {
  label: string;
  /** 0–100. */
  usedPercent: number;
  /** Epoch ms. */
  resetsAt: number | null;
}

/** Subscription usage limits for one agent. */
export interface ProviderUsage {
  kind: AgentKind;
  plan: string | null;
  windows: UsageWindow[];
  /** Why no windows are shown, e.g. "Not signed in". */
  error: string | null;
  updatedAt: number | null;
}

/** Where imported conversations come from: an agent CLI, another app, or OpenCode. */
export type ImportSource = CliKind | "t3";

/** Conversations a source saved on disk that can be imported. */
export interface ImportScan {
  kind: ImportSource;
  found: number;
  /** Not yet in Patchwork. */
  new: number;
  /** Sub-agent runs, which are not offered for import. */
  subagents: number;
}

export type AgentEvent =
  | { type: "goalUpdated"; goal: Goal | null }
  | { type: "goalChecking"; messageId: string }
  | { type: "retrying"; messageId: string; reason: string; attempt: number; maxAttempts: number }
  | { type: "slashCommandsUpdated"; commands: ProviderSlashCommand[] }
  | { type: "roleChanged"; role: string | null }
  | { type: "modelChanged"; model: string | null }
  | { type: "usage"; messageId: string; usage: TokenUsage }
  | { type: "messageStarted"; message: Message }
  | { type: "textDelta"; messageId: string; text: string }
  | {
      type: "thinking";
      messageId: string;
      text: string;
      startedAt?: number;
      endedAt?: number;
    }
  | { type: "toolStarted"; messageId: string; tool: ToolCall }
  | {
      type: "toolFinished";
      messageId: string;
      toolId: string;
      isError: boolean;
      output?: string;
    }
  | { type: "planUpdated"; messageId: string; steps: PlanStep[] }
  | { type: "fileTouched"; messageId: string; path: string }
  | { type: "approvalRequested"; messageId: string; approval: Approval }
  | {
      type: "approvalResolved";
      messageId: string;
      approvalId: string;
      allowed: boolean;
      answers?: Record<string, string[]>;
      rule?: RuleRef;
      auto?: boolean;
    }
  | { type: "messageQueued"; messageId: string; queued: boolean }
  | { type: "messageSteered"; messageId: string }
  | { type: "messageCutOff"; messageId: string }
  | { type: "refused"; messageId: string; refusal: Refusal }
  | { type: "planProposed"; messageId: string; text: string }
  | { type: "threadCreated"; thread: Thread }
  | { type: "titleUpdated"; title: string }
  | { type: "messageRemoved"; messageId: string }
  | { type: "messageEdited"; message: Message }
  | { type: "backgroundTaskStarted"; messageId: string; task: BackgroundTask }
  | { type: "backgroundTaskEnded"; messageId: string; toolId: string }
  | { type: "turnRecorded"; messageId: string; turn: TurnTrees }
  | {
      type: "turnCompleted";
      messageId: string;
      status: Exclude<MessageStatus, "streaming">;
      error: string | null;
    };

export interface AgentEventEnvelope {
  threadId: string;
  event: AgentEvent;
}

export const DEFAULT_SETTINGS: Settings = {
  onboarded: false,
  theme: "dark",
  agents: [],
  integrations: { github: false, terminal: false, figma: false, browser: false },
  projects: [],
  activeProjectId: null,
  agentPaths: {},
  shell: null,
  skillsDir: null,
  disabledSkills: [],
  tokens: {
    showUsage: true,
    warnAbove: 150_000,
    askBeforeResume: true,
    autoCompact: true,
    autoCompactAbove: 0,
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
  /** Every place an agent takes standing instructions from, as a tree. */
  memoryTree: () => invoke<MemoryNode[]>("memory_tree"),
  readMemory: (path: string) => invoke<string>("read_memory", { path }),
  openMemoryFile: (path: string) => invoke<void>("open_memory_file", { path }),
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

/** A settled burst of file changes in the watched project, with git's state after it. */
export interface ProjectChanged {
  projectId: string;
  /** Project-relative paths the burst touched; `.git/...` for commits, staging and checkouts. */
  paths: string[];
  branch: string | null;
  /** Null when `git status` failed. */
  changes: FileChange[] | null;
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
