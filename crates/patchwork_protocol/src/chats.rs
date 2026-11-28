//! Chats as clients see them, and the params that pick or make one.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};

/// What an agent may do without asking. The protocol writes it by the names clients use
/// (`autoAcceptEdits`), through [`patchwork_model::chats::client_access_mode`].
pub use patchwork_model::chats::{AccessMode, ChatStatus};

/// A chat, as the web client's `Chat` type has it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chat {
    pub id: String,
    pub project_id: Option<String>,
    pub title: String,
    /// The agent's id, such as `claude`.
    pub agent: String,
    pub model: Option<String>,
    #[serde(with = "patchwork_model::chats::client_access_mode")]
    pub access_mode: AccessMode,
    pub session_id: Option<String>,
    pub effort: Option<String>,
    pub status: ChatStatus,
    pub position: i64,
    pub created_at: u64,
    pub updated_at: u64,
    pub archived_at: Option<u64>,
    pub pinned: bool,
    /// Always null: the app does not run chats in worktrees of their own.
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    pub checkpoint_ref: Option<String>,
}

/// Params naming one chat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatParams {
    pub chat_id: String,
}

/// The params of `chats.create`; what is left out comes from the settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewChatParams {
    pub project_id: Option<String>,
    /// The agent's id; the default agent when left out.
    pub agent: Option<String>,
    pub model: Option<String>,
    /// An [`AccessMode`] name, read with [`AccessMode::named`] so a wrong one is named back.
    pub access_mode: Option<String>,
    pub effort: Option<String>,
}
