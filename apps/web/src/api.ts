// Typed bridge to the Rust backend. Mirrors apps/server/src/types.rs and the
// commands registered in apps/desktop/src-tauri/src/lib.rs — keep all three in sync.
import { invoke as tauriInvoke, type InvokeArgs } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { profile } from "~/lib/profile";

const invoke = <T>(cmd: string, args?: InvokeArgs) =>
  profile(`ipc:${cmd}`, () => tauriInvoke<T>(cmd, args));

export type AgentKind = "claude" | "codex" | "antigravity" | "opencode";
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
  integrations: Integrations;
  projects: Project[];
  activeProjectId: string | null;
  tokens: TokenSettings;
  /** Shell that agents run commands in; null follows the SHELL environment variable. */
  shell: string | null;
  notifications: NotificationSettings;
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
  /** The context size above which an idle task is compacted. */
  autoCompactAbove: number;
  /** Model for new tasks, per agent. A missing agent uses the CLI's own default. */
  defaultModels: Partial<Record<AgentKind, string>>;
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
  | { type: "plan"; text: string };

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
  /** A safety classifier blocked part of the reply; the agent's stated reason. */
  refusal: Refusal | null;
  /** The reply's text, reasoning, and tool calls in the order they happened; empty for replies
   * saved before this was kept. */
  parts: Part[];
  /** When the reply finished (ms since epoch); null while it runs. */
  finishedAt: number | null;
  /** Tokens the reply used; null for agents that do not report usage. */
  usage: TokenUsage | null;
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
export type ImportSource = AgentKind | "t3" | "opencode";

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
  | { type: "slashCommandsUpdated"; commands: ProviderSlashCommand[] }
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
    }
  | { type: "messageQueued"; messageId: string; queued: boolean }
  | { type: "messageSteered"; messageId: string }
  | { type: "messageCutOff"; messageId: string }
  | { type: "refused"; messageId: string; refusal: Refusal }
  | { type: "planProposed"; messageId: string; text: string }
  | { type: "threadCreated"; thread: Thread }
  | { type: "messageRemoved"; messageId: string }
  | { type: "messageEdited"; message: Message }
  | { type: "backgroundTaskStarted"; messageId: string; task: BackgroundTask }
  | { type: "backgroundTaskEnded"; messageId: string; toolId: string }
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
  shell: null,
  tokens: {
    showUsage: true,
    warnAbove: 150_000,
    askBeforeResume: true,
    autoCompact: true,
    autoCompactAbove: 200_000,
    defaultModels: {},
  },
  notifications: { goalComplete: true, taskComplete: true },
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
};

export const SOURCE_LABELS: Record<ImportSource, string> = {
  ...AGENT_LABELS,
  t3: "T3 Code",
  opencode: "OpenCode",
};

/** Import sources to offer: each connected agent's own files, plus another app and OpenCode. */
export function importSourcesFor(agents: AgentKind[]): ImportSource[] {
  return [...agents, "t3", "opencode"];
}

const agentStatusCache: Partial<Record<AgentKind, AgentStatus>> = {};
const pendingDetect: Partial<Record<AgentKind, Promise<AgentStatus>>> = {};

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
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
  listModels: (kind: AgentKind) => invoke<ModelOption[]>("list_models", { kind }),
  /** Installed login shells, as paths from /etc/shells, and the default shell's path. */
  listShells: () => invoke<ShellChoices>("list_shells"),
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
  savePastedImage: (mimeType: string, data: string) =>
    invoke<Attachment>("save_pasted_image", { mimeType, data }),
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
  ) => invoke<void>("respond_approval", { threadId, approvalId, allow, answers }),

  readFile: (projectId: string, path: string) => invoke<string>("read_file", { projectId, path }),
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
  /** Starts watching the project's files; `onProjectFilesChanged` then reports changes. */
  watchProject: (projectId: string) => invoke<void>("watch_project", { projectId }),
};

export function onAgentEvent(cb: (e: AgentEventEnvelope) => void): Promise<UnlistenFn> {
  return listen<AgentEventEnvelope>("agent-event", (e) => cb(e.payload));
}

export function onProjectFilesChanged(cb: (projectId: string) => void): Promise<UnlistenFn> {
  return listen<string>("project-files-changed", (e) => cb(e.payload));
}
