//! The chats in the store, as the task list reads them.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use patchwork_model::chats::Chat;

use super::AppState;

impl AppState {
    /// Reads the chats, the folders and the chat apps again.
    ///
    /// Returns whether the agents should be read again too: a chat app's first message makes its
    /// agent, and an agent's new chat may be its chat.
    pub fn reload_chats(&mut self) -> bool {
        let chats = self.core.list_chats();
        self.reload_folders();
        self.reload_chat_apps();
        let new_agent_chat = chats
            .iter()
            .any(|t| t.agent_id.is_some() && !self.chats.iter().any(|known| known.id == t.id));
        let unknown_agent = chats.iter().any(|t| {
            t.agent_id
                .as_ref()
                .is_some_and(|id| !self.agents.iter().any(|b| &b.id == id))
        });
        self.chats = Arc::new(chats);
        new_agent_chat || unknown_agent
    }

    /// Chat `id` as the task list has it.
    #[must_use]
    pub fn chat(&self, id: &str) -> Option<&Arc<Chat>> {
        self.chats.iter().find(|t| t.id == id)
    }
}
