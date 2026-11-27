//! Chats as clients see them, and the params that pick or make one.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};

/// What an agent may do without asking, by the names clients use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AccessMode {
    Supervised,
    AutoAcceptEdits,
    Auto,
    FullAccess,
}

impl AccessMode {
    /// The mode a client calls `name`, if any.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "supervised" => Some(Self::Supervised),
            "autoAcceptEdits" => Some(Self::AutoAcceptEdits),
            "auto" => Some(Self::Auto),
            "fullAccess" => Some(Self::FullAccess),
            _ => None,
        }
    }
}

/// Whether a chat is working, failed or waiting for the person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatStatus {
    /// Its agent's process is running a turn.
    Running,
    /// Its last message failed.
    Error,
    Idle,
}

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

#[cfg(test)]
mod tests {
    use super::{AccessMode, ChatStatus};

    #[test]
    fn access_modes_read_the_names_they_write() {
        for mode in [
            AccessMode::Supervised,
            AccessMode::AutoAcceptEdits,
            AccessMode::Auto,
            AccessMode::FullAccess,
        ] {
            let name = serde_json::to_value(mode).expect("a name");
            assert_eq!(
                AccessMode::named(name.as_str().expect("a string")),
                Some(mode)
            );
        }
        assert_eq!(AccessMode::named("wild"), None);
    }

    #[test]
    fn statuses_have_the_client_names() {
        let names = [ChatStatus::Running, ChatStatus::Error, ChatStatus::Idle]
            .map(|s| serde_json::to_string(&s).expect("a name"));
        assert_eq!(names, [r#""running""#, r#""error""#, r#""idle""#]);
    }
}
