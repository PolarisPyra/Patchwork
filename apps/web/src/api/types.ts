import type { AccessMode } from "~/bindings/AccessMode";
import type { AgentEvent } from "~/bindings/AgentEvent";
import type { AgentEventEnvelope } from "~/bindings/AgentEventEnvelope";
import type { AgentKind } from "~/bindings/AgentKind";
import type { AgentStatus } from "~/bindings/AgentStatus";
import type { ApiModel } from "~/bindings/ApiModel";
import type { ApiProvider } from "~/bindings/ApiProvider";
import type { Approval } from "~/bindings/Approval";
import type { ApprovalStatus } from "~/bindings/ApprovalStatus";
import type { Attachment } from "~/bindings/Attachment";
import type { AttachmentKind } from "~/bindings/AttachmentKind";
import type { BackgroundTask } from "~/bindings/BackgroundTask";
import type { ClaudeInfo } from "~/bindings/ClaudeInfo";
import type { CodexFeature } from "~/bindings/CodexFeature";
import type { CodexInfo } from "~/bindings/CodexInfo";
import type { CodexModel } from "~/bindings/CodexModel";
import type { Effect } from "~/bindings/Effect";
import type { FileChange } from "~/bindings/FileChange";
import type { FileMedia } from "~/bindings/FileMedia";
import type { Folder } from "~/bindings/Folder";
import type { Goal } from "~/bindings/Goal";
import type { GoalStatus } from "~/bindings/GoalStatus";
import type { ImportScan } from "~/bindings/ImportScan";
import type { ImportSource } from "~/bindings/ImportSource";
import type { Integrations } from "~/bindings/Integrations";
import type { LogChunk } from "~/bindings/LogChunk";
import type { LoggingSettings } from "~/bindings/LoggingSettings";
import type { LogLevel } from "~/bindings/LogLevel";
import type { McpServer } from "~/bindings/McpServer";
import type { Message as WireMessage } from "~/bindings/Message";
import type { MessageStatus } from "~/bindings/MessageStatus";
import type { ModelOption } from "~/bindings/ModelOption";
import type { NotificationSettings } from "~/bindings/NotificationSettings";
import type { Part } from "~/bindings/Part";
import type { PermissionKind } from "~/bindings/PermissionKind";
import type { PermissionRule } from "~/bindings/PermissionRule";
import type { PermissionScope } from "~/bindings/PermissionScope";
import type { PermissionSettings } from "~/bindings/PermissionSettings";
import type { PlanStep } from "~/bindings/PlanStep";
import type { Plugin } from "~/bindings/Plugin";
import type { PluginApp } from "~/bindings/PluginApp";
import type { PluginHook } from "~/bindings/PluginHook";
import type { PluginHookHandler } from "~/bindings/PluginHookHandler";
import type { Project } from "~/bindings/Project";
import type { ProjectChanged } from "~/bindings/ProjectChanged";
import type { ProviderSlashCommand } from "~/bindings/ProviderSlashCommand";
import type { ProviderUsage } from "~/bindings/ProviderUsage";
import type { Question } from "~/bindings/Question";
import type { Refusal } from "~/bindings/Refusal";
import type { Role } from "~/bindings/Role";
import type { RuleRef } from "~/bindings/RuleRef";
import type { RunOutcome } from "~/bindings/RunOutcome";
import type { Schedule } from "~/bindings/Schedule";
import type { ScheduledRun } from "~/bindings/ScheduledRun";
import type { ScheduledTask } from "~/bindings/ScheduledTask";
import type { ScopeRules } from "~/bindings/ScopeRules";
import type { Screenshot } from "~/bindings/Screenshot";
import type { Settings } from "~/bindings/Settings";
import type { ShellChoices } from "~/bindings/ShellChoices";
import type { Skill } from "~/bindings/Skill";
import type { StepStatus } from "~/bindings/StepStatus";
import type { SubAgent } from "~/bindings/SubAgent";
import type { TerminalSettings } from "~/bindings/TerminalSettings";
import type { Theme } from "~/bindings/Theme";
import type { Thread as WireThread } from "~/bindings/Thread";
import type { TokenSettings } from "~/bindings/TokenSettings";
import type { TokenUsage } from "~/bindings/TokenUsage";
import type { ToolCall } from "~/bindings/ToolCall";
import type { TurnFile } from "~/bindings/TurnFile";
import type { TurnFileState } from "~/bindings/TurnFileState";
import type { TurnTrees } from "~/bindings/TurnTrees";
import type { UsageWindow } from "~/bindings/UsageWindow";
import type { Weekday } from "~/bindings/Weekday";
export type {
  AccessMode,
  AgentEvent,
  AgentEventEnvelope,
  AgentKind,
  AgentStatus,
  ApiModel,
  ApiProvider,
  Approval,
  ApprovalStatus,
  Attachment,
  AttachmentKind,
  BackgroundTask,
  ClaudeInfo,
  CodexFeature,
  CodexInfo,
  CodexModel,
  Effect,
  FileChange,
  FileMedia,
  Folder,
  Goal,
  GoalStatus,
  ImportScan,
  ImportSource,
  Integrations,
  LogChunk,
  LoggingSettings,
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
  Screenshot,
  Settings,
  ShellChoices,
  Skill,
  StepStatus,
  SubAgent,
  TerminalSettings,
  Theme,
  TokenSettings,
  TokenUsage,
  ToolCall,
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
  /** The agent is retrying a model request that failed or stalled; not saved. */
  retry?: { reason: string; attempt: number; maxAttempts: number } | null;
};

/** A thread as the webview holds it. Set in the UI on a thread from `listThreads`, which carries only its last message and those still streaming, queued, or awaiting approval; `getThread` loads the rest. */
export type Thread = Omit<WireThread, "messages"> & { messages: Message[]; partial?: boolean };
