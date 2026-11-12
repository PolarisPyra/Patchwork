//! Threads, their goals, and how much they may do unasked.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};

use crate::provider::{AgentKind, ProviderSlashCommand};
use crate::backgrounds::ChatBackground;
use crate::threads::{Message, MessageStatus, Role};

/// How much an agent may do without asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate fact about the chat, stored as its own column"
)]
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
    /// The chat's own background. None shows the one in Settings, as does any background while
    /// the plan lacks per-chat backgrounds.
    #[serde(default)]
    pub background: Option<ChatBackground>,
    /// Claude `session_id` / Codex `thread_id` used to resume the conversation.
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
    /// Listed under Favorites at the top of the sidebar, in place of its usual spot.
    #[serde(default)]
    pub favorite: bool,
    /// A task listed first in its project, above its folders. Chats without a project are never
    /// pinned.
    #[serde(default)]
    pub pinned: bool,
}

/// Where a subagent's thread came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadParent {
    pub thread_id: String,
    pub tool_id: String,
}

/// Where a Claude chat's next turn picks its session up, in place of the session's newest
/// message: a reply kept by an undo, or the reply a fork branches from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumePoint {
    /// The cursor of the reply the session goes on from.
    pub at: String,
    /// Branches the session into a new one and leaves the original as it is, as a fork does.
    pub fork: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// The error for a chat or task that no longer exists. People see chat for either.
pub(crate) const NOT_FOUND: &str = "Chat not found";

impl Thread {
    /// The thread with `messages` in place of its own, which are not copied on the way: a
    /// thread's history can be megabytes, and the list of every thread is made on each change.
    #[must_use]
    pub(crate) fn with_messages(&self, messages: Vec<Message>) -> Thread {
        Thread {
            id: self.id.clone(),
            project_id: self.project_id.clone(),
            title: self.title.clone(),
            agent: self.agent,
            model: self.model.clone(),
            access_mode: self.access_mode,
            effort: self.effort.clone(),
            compact_window: self.compact_window,
            background: self.background.clone(),
            session_id: self.session_id.clone(),
            created_at: self.created_at,
            messages,
            goal: self.goal.clone(),
            position: self.position,
            folder_id: self.folder_id.clone(),
            archived_at: self.archived_at,
            parent: self.parent.clone(),
            plan_mode: self.plan_mode,
            slash_commands: self.slash_commands.clone(),
            imported_from: self.imported_from.clone(),
            context_pending: self.context_pending,
            role: self.role.clone(),
            role_pending: self.role_pending,
            favorite: self.favorite,
            pinned: self.pinned,
        }
    }

    /// The reply the app's closing cut off, while it is the last thing sent in the thread: once
    /// anything follows it the conversation has moved on, and there is nothing to resume. A
    /// message still queued waits behind the reply, so it does not count.
    #[must_use]
    pub fn interrupted_reply(&self) -> Option<&Message> {
        self.messages
            .iter()
            .rfind(|m| !m.queued)
            .filter(|m| m.role == Role::Assistant && m.status == MessageStatus::Interrupted)
    }

    /// The reply whose turn can be carried on in the agent's session: the last thing sent in the
    /// thread, when it was cut off by the app's closing or ended with an error. A subagent's
    /// thread has none, since its turn belongs to the thread that started it.
    #[must_use]
    pub fn resumable_reply(&self) -> Option<&Message> {
        self.messages
            .iter()
            .rfind(|m| !m.queued)
            .filter(|_| self.parent.is_none())
            .filter(|m| m.role == Role::Assistant)
            .filter(|m| matches!(m.status, MessageStatus::Interrupted | MessageStatus::Error))
    }
}
