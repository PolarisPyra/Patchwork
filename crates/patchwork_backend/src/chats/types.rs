//! Chats, their goals, and how much they may do unasked.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::provider::{AgentKind, ProviderSlashCommand};
use crate::backgrounds::ChatBackground;
use crate::chats::{Message, MessageStatus, Part, Role};

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
pub struct Chat {
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
    /// The context size at which this chat's agent compacts mid-task. None follows Settings →
    /// Tokens; 0 leaves it until the context is full.
    #[serde(default)]
    pub compact_window: Option<u64>,
    /// The chat's own background. None shows the one in Settings, as does any background while
    /// the plan lacks per-chat backgrounds.
    #[serde(default)]
    pub background: Option<ChatBackground>,
    /// Claude `session_id` / Codex `chat_id` used to resume the conversation.
    pub session_id: Option<String>,
    pub created_at: u64,
    /// Shared, so copying a chat copies no message: a history can be megabytes.
    pub messages: Vec<Arc<Message>>,
    /// The `/goal` the agent is working toward, as the agent last reported it.
    #[serde(default)]
    pub goal: Option<Goal>,
    /// Sidebar order, lower first. Kept by the store; the UI gets chats already sorted.
    #[serde(skip)]
    pub position: i64,
    /// The folder this task is in; None for the project's top level.
    #[serde(default)]
    pub folder_id: Option<String>,
    /// When the chat was archived; None while it is in the task list.
    #[serde(default)]
    pub archived_at: Option<u64>,
    /// Set on a subagent's chat: the chat and call that started it. Such a chat is
    /// read-only and not listed in the sidebar.
    #[serde(default)]
    pub parent: Option<ChatParent>,
    /// Provider-native commands discovered from the CLI's initialization frame.
    #[serde(default)]
    pub slash_commands: Vec<ProviderSlashCommand>,
    /// The OpenCode session this chat was imported from, so a re-import does not add it again.
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
    /// The agent this chat is a run of; None for a chat or task started by hand.
    #[serde(default)]
    pub agent_id: Option<String>,
}

/// Where a subagent's chat came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatParent {
    pub chat_id: String,
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
    /// Paused by the user, or by a reply that failed or was stopped.
    Paused,
    /// Paused after replies in a row that did no work toward it.
    Stalled,
    Blocked,
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
    /// Replies that ended with the goal still unmet.
    pub checks: Option<u32>,
    /// Seconds spent pursuing the goal before it was last made active.
    pub time_used_seconds: Option<u64>,
    /// Unix epoch milliseconds when the goal was last made active; the start when unset.
    pub active_since: Option<u64>,
}

impl Goal {
    /// The goal with `status` at `now` (Unix epoch milliseconds). Its time stops when it stops
    /// being pursued, and runs again when it is taken up.
    #[must_use]
    pub fn with_status(self, status: GoalStatus, now: u64) -> Self {
        let was_active = self.status == GoalStatus::Active;
        let active = status == GoalStatus::Active;
        let (time_used_seconds, active_since) = match (was_active, active) {
            (true, false) => (Some(self.seconds_used(now)), None),
            (false, true) => (self.time_used_seconds, Some(now)),
            _ => (self.time_used_seconds, self.active_since),
        };
        Self {
            status,
            time_used_seconds,
            active_since,
            ..self
        }
    }

    /// Seconds spent pursuing the goal by `now` (Unix epoch milliseconds).
    #[must_use]
    pub fn seconds_used(&self, now: u64) -> u64 {
        let before = self.time_used_seconds.unwrap_or(0);
        if self.status == GoalStatus::Active {
            let since = self.active_since.unwrap_or(self.started_at);
            before + now.saturating_sub(since) / 1000
        } else {
            before
        }
    }
}

/// The error for a chat or task that no longer exists. People see chat for either.
pub(crate) const NOT_FOUND: &str = "Chat not found";

impl Chat {
    /// A chat with no messages, no project, parent or session, and the agent's own settings.
    #[must_use]
    pub(crate) fn new(title: String, agent: AgentKind, access_mode: AccessMode) -> Chat {
        Chat {
            id: crate::system::new_id(),
            project_id: None,
            title,
            agent,
            model: None,
            access_mode,
            effort: None,
            compact_window: None,
            background: None,
            session_id: None,
            created_at: crate::system::now_ms(),
            messages: Vec::new(),
            goal: None,
            position: 0,
            folder_id: None,
            archived_at: None,
            parent: None,
            slash_commands: Vec::new(),
            imported_from: None,
            context_pending: false,
            role: None,
            role_pending: false,
            favorite: false,
            pinned: false,
            agent_id: None,
        }
    }

    /// The chat with `messages` in place of its own, which are not copied on the way: a
    /// chat's history can be megabytes, and the list of every chat is made on each change.
    #[must_use]
    pub(crate) fn with_messages(&self, messages: Vec<Arc<Message>>) -> Chat {
        Chat {
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
            slash_commands: self.slash_commands.clone(),
            imported_from: self.imported_from.clone(),
            context_pending: self.context_pending,
            role: self.role.clone(),
            role_pending: self.role_pending,
            favorite: self.favorite,
            pinned: self.pinned,
            agent_id: self.agent_id.clone(),
        }
    }

    /// The reply the app's closing cut off, while it is the last thing sent in the chat: once
    /// anything follows it the conversation has moved on, and there is nothing to resume. A
    /// message still queued waits behind the reply, so it does not count.
    #[must_use]
    pub fn interrupted_reply(&self) -> Option<&Message> {
        self.messages
            .iter()
            .map(Arc::as_ref)
            .rfind(|m| !m.queued)
            .filter(|m| m.role == Role::Assistant && m.status == MessageStatus::Interrupted)
    }

    /// The reply whose turn can be carried on in the agent's session: the last thing sent in the
    /// chat, when it was cut off by the app's closing or ended with an error. A subagent's
    /// chat has none, since its turn belongs to the chat that started it.
    #[must_use]
    pub fn resumable_reply(&self) -> Option<&Message> {
        self.messages
            .iter()
            .map(Arc::as_ref)
            .rfind(|m| !m.queued)
            .filter(|_| self.parent.is_none())
            .filter(|m| m.role == Role::Assistant)
            .filter(|m| matches!(m.status, MessageStatus::Interrupted | MessageStatus::Error))
    }

    /// How large the context may grow, as the agent last reported it.
    #[must_use]
    pub fn context_window(&self) -> Option<u64> {
        self.messages
            .iter()
            .rev()
            .find_map(|m| m.usage.as_ref()?.context_window)
    }

    /// Current context size in tokens, reset after a completed `/compact`.
    #[must_use]
    pub fn current_context(&self) -> Option<u64> {
        let mut latest = None;
        let mut after_compact = false;
        for m in &self.messages {
            if m.queued {
                continue;
            }
            if m.role == Role::User && crate::provider::is_compact_command(&m.text) {
                after_compact = true;
                continue;
            }
            if after_compact {
                after_compact = false;
                if m.role == Role::Assistant && m.status == MessageStatus::Complete {
                    // A completed /compact clears the pre-compaction context. Keep the reply's
                    // context only when it reflects post-compaction usage (such as an ACP
                    // `usage_update`), not the pre-compaction input tokens read to summarize it.
                    // Antigravity reports the size from before as `/compact` ends, and the
                    // smaller one only on its next reply.
                    let before = latest;
                    latest = m
                        .usage
                        .as_ref()
                        .filter(|u| u.context > 0 && (u.input == 0 || u.context < u.input))
                        .filter(|u| before.is_none_or(|b| u.context < b))
                        .map(|u| u.context);
                    continue;
                }
            }
            // A completed mid-turn compaction resets context until the continuation reports a
            // post-compaction `usage_update`.
            if m.role == Role::Assistant
                && m.parts
                    .iter()
                    .any(|p| matches!(p, Part::Compaction { done: true, .. }))
            {
                latest = m
                    .usage
                    .as_ref()
                    .filter(|u| u.context > 0)
                    .map(|u| u.context);
                continue;
            }
            // A reply that never said how full the context is leaves it as it was.
            if let Some(usage) = m.usage.as_ref().filter(|u| u.context > 0) {
                latest = Some(usage.context);
            }
        }
        latest
    }
}

/// What filled the window when the app last closed, to open again at the next launch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "camelCase")]
pub enum LastPlace {
    /// The orchestrator page.
    Orchestrator,
    /// A chat, by its id.
    Chat(String),
    /// An agent's page, by the agent's id.
    Agent(String),
}
