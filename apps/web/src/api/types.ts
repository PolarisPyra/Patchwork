import type { Account, AccountDevice, LoginStep } from "~/bindings/accounts";
import type { ChatBackground } from "~/bindings/backgrounds";
import type { Screenshot } from "~/bindings/computer";
import type { ImportScan, ImportSource } from "~/bindings/import";
import type { Feature, LicenseStatus, Plan } from "~/bindings/license";
import type { LogChunk } from "~/bindings/logging";
import type {
  AgentEvent,
  AgentEventEnvelope,
  TurnFile,
  TurnFileState,
} from "~/bindings/orchestration";
import type {
  Effect,
  PermissionKind,
  PermissionRule,
  PermissionScope,
  PermissionSettings,
  RuleRef,
  ScopeRules,
} from "~/bindings/permissions";
import type { Plugin, PluginApp, PluginHook, PluginHookHandler } from "~/bindings/plugins";
import type { Folder, Project } from "~/bindings/projects";
import type {
  AgentKind,
  AgentStatus,
  ApiModel,
  ApiProvider,
  ClaudeInfo,
  CodexFeature,
  CodexInfo,
  CodexModel,
  McpServer,
  ModelOption,
  ProviderSlashCommand,
} from "~/bindings/provider";
import type {
  RunOutcome,
  Schedule,
  ScheduledRun,
  ScheduledTask,
  Weekday,
} from "~/bindings/scheduled";
import type {
  CheckSettings,
  Integrations,
  LoggingSettings,
  LogLevel,
  NotificationSettings,
  Settings,
  TerminalSettings,
  Theme,
  TokenSettings,
} from "~/bindings/settings";
import type { Secret, SecretStatus } from "~/bindings/secrets";
import type { Skill } from "~/bindings/skills";
import type {
  AttachedTerminal,
  TerminalFrame,
  TerminalInfo,
  TerminalShell,
} from "~/bindings/terminal";
import type {
  AccessMode,
  Approval,
  ApprovalStatus,
  Attachment,
  AttachmentKind,
  BackgroundTask,
  CheckFailure,
  CheckStatus,
  Goal,
  GoalStatus,
  Message as WireMessage,
  MessageStatus,
  Part,
  PlanStep,
  Question,
  Refusal,
  Role,
  StepStatus,
  SubAgent,
  Thread as WireThread,
  TokenUsage,
  ToolCall,
  TurnChecks,
  TurnTrees,
} from "~/bindings/threads";
import type { ProviderUsage, UsageWindow } from "~/bindings/usage";
import type { FileChange, FileMedia, ProjectChanged } from "~/bindings/workspace";
export type {
  Account,
  AccountDevice,
  AccessMode,
  AgentEvent,
  AgentEventEnvelope,
  AgentKind,
  AgentStatus,
  ApiModel,
  ApiProvider,
  Approval,
  ApprovalStatus,
  AttachedTerminal,
  Attachment,
  AttachmentKind,
  BackgroundTask,
  ChatBackground,
  CheckFailure,
  CheckSettings,
  CheckStatus,
  ClaudeInfo,
  CodexFeature,
  CodexInfo,
  CodexModel,
  Effect,
  Feature,
  FileChange,
  FileMedia,
  Folder,
  Goal,
  GoalStatus,
  ImportScan,
  ImportSource,
  Integrations,
  LicenseStatus,
  LogChunk,
  LoggingSettings,
  LoginStep,
  LogLevel,
  McpServer,
  MessageStatus,
  ModelOption,
  NotificationSettings,
  Part,
  PermissionKind,
  PermissionRule,
  PermissionScope,
  PermissionSettings,
  Plan,
  PlanStep,
  Plugin,
  PluginApp,
  PluginHook,
  PluginHookHandler,
  Project,
  ProjectChanged,
  ProviderSlashCommand,
  ProviderUsage,
  Question,
  Refusal,
  Role,
  RuleRef,
  RunOutcome,
  Schedule,
  ScheduledRun,
  ScheduledTask,
  ScopeRules,
  Secret,
  SecretStatus,
  Screenshot,
  Settings,
  Skill,
  StepStatus,
  SubAgent,
  TerminalFrame,
  TerminalInfo,
  TerminalSettings,
  TerminalShell,
  Theme,
  TokenSettings,
  TokenUsage,
  ToolCall,
  TurnChecks,
  TurnFile,
  TurnFileState,
  TurnTrees,
  UsageWindow,
  Weekday,
};

/**
 * A message as the webview holds it: what the backend saves, plus what a reply adds in the UI while
 * it runs.
 */
export type Message = WireMessage & {
  /** The agent is checking whether the goal is met; not saved. */
  checkingGoal?: boolean;
  /** The agent is summarizing the conversation to free up context; not saved. */
  compacting?: boolean;
  /** The agent is retrying a model request that failed or stalled; not saved. */
  retry?: { reason: string; attempt: number; maxAttempts: number } | null;
};

/** A thread as the webview holds it. Set in the UI on a thread from `listThreads`, which carries only its last message and those still streaming, queued, or awaiting approval; `getThread` loads the rest. */
export type Thread = Omit<WireThread, "messages"> & { messages: Message[]; partial?: boolean };
