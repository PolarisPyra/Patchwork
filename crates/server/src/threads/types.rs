//! Threads, their goals, and how much they may do unasked.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::provider::{AgentKind, ProviderSlashCommand};
use crate::threads::Message;

/// How much an agent may do without asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, TS)]
#[ts(export, export_to = "threads.ts")]
#[serde(rename_all = "kebab-case")]
pub enum AccessMode {
    /// Ask before commands and file changes.
    Supervised,
    /// Edits are approved; other actions ask.
    AutoAcceptEdits,
    /// The provider approves routine actions; others still ask.
    Auto,
    #[default]
    FullAccess,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "threads.ts")]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: String,
    /// None for a chat without a project; it runs in a private scratch folder.
    pub project_id: Option<String>,
    pub title: String,
    pub agent: AgentKind,
    pub model: Option<String>,
    #[serde(default)]
    pub access_mode: AccessMode,
    /// Reasoning effort passed to the agent, e.g. "high"; None leaves the CLI's default.
    #[serde(default)]
    pub effort: Option<String>,
    /// The context size at which this thread's agent compacts mid-task. None follows Settings →
    /// Tokens; 0 leaves it until the context is full.
    #[serde(default)]
    pub compact_window: Option<u64>,
    /// Claude session_id / Codex thread_id used to resume the conversation.
    pub session_id: Option<String>,
    pub created_at: u64,
    pub messages: Vec<Message>,
    /// The `/goal` the agent is working toward, as the agent last reported it.
    #[serde(default)]
    pub goal: Option<Goal>,
    /// Sidebar order, lower first. Kept by the store; the UI gets threads already sorted.
    #[serde(skip)]
    pub position: i64,
    /// The folder this task is in; None for the project's top level.
    #[serde(default)]
    pub folder_id: Option<String>,
    /// When the thread was archived; None while it is in the task list.
    #[serde(default)]
    pub archived_at: Option<u64>,
    /// Set on a subagent's thread: the thread and call that started it. Such a thread is
    /// read-only and not listed in the sidebar.
    #[serde(default)]
    pub parent: Option<ThreadParent>,
    /// Turns run in plan mode: the agent plans and proposes, without changing files.
    #[serde(default)]
    pub plan_mode: bool,
    /// Provider-native commands discovered from the CLI's initialization frame.
    #[serde(default)]
    pub slash_commands: Vec<ProviderSlashCommand>,
    /// The OpenCode session this thread was imported from, so a re-import does not add it again.
    #[serde(default)]
    pub imported_from: Option<String>,
    /// An imported chat whose transcript the next turn sends as context, since no agent session
    /// holds it yet. Cleared once a turn has started.
    #[serde(default)]
    pub context_pending: bool,
    /// The primary custom agent the chat is in the role of (see `agents`).
    #[serde(default)]
    pub role: Option<String>,
    /// The role changed since the agent was last told of it, by hand rather than by summoning.
    #[serde(skip)]
    pub role_pending: bool,
}

/// Where a subagent's thread came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "threads.ts")]
#[serde(rename_all = "camelCase")]
pub struct ThreadParent {
    pub thread_id: String,
    pub tool_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "threads.ts")]
#[serde(rename_all = "camelCase")]
pub enum GoalStatus {
    Active,
    Paused,
    Blocked,
    UsageLimited,
    BudgetLimited,
    Complete,
}

/// A condition the agent keeps working toward until it is met (`/goal`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "threads.ts")]
#[serde(rename_all = "camelCase")]
pub struct Goal {
    pub objective: String,
    pub status: GoalStatus,
    /// Unix epoch milliseconds.
    pub started_at: u64,
    /// Evaluator checks that came back unmet (Claude).
    pub checks: Option<u32>,
    /// What the agent reports having spent on the goal (Codex).
    pub tokens_used: Option<u64>,
    pub token_budget: Option<u64>,
    pub time_used_seconds: Option<u64>,
}
