//! What making a chat needs beyond its data: a new id and the time. The types live in
//! `patchwork_model`.

// Rust guideline compliant 2026-11-27

use crate::provider::AgentKind;
use patchwork_model::chats::{AccessMode, Chat};

/// The error for a chat or task that no longer exists. People see chat for either.
pub(crate) const NOT_FOUND: &str = "Chat not found";

/// Making a chat.
pub(crate) trait ChatExt {
    /// A chat with no messages, no project, parent or session, and the agent's own settings.
    fn new(title: String, agent: AgentKind, access_mode: AccessMode) -> Self;
}

impl ChatExt for Chat {
    fn new(title: String, agent: AgentKind, access_mode: AccessMode) -> Self {
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
}
