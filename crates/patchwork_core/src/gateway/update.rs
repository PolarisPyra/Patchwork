//! The conversations told once Patchwork is back from an update one of them asked for.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};

use super::types::Binding;
use crate::{Core, Error};

/// The app state key of the conversations to tell once Patchwork is back from an update.
pub const AFTER_UPDATE: &str = "gateway.afterUpdate";

/// Who asked for an update from a chat app, kept across the restart into it.
#[derive(Debug, Serialize, Deserialize)]
pub struct AfterUpdate {
    /// The version that was running.
    pub from: String,
    /// The conversations to tell.
    #[serde(rename = "chats")]
    pub conversations: Vec<Binding>,
}

impl Core {
    /// Notes the conversations of the chat `chat_id` to tell once Patchwork is back from the
    /// update it is about to restart into.
    ///
    /// # Errors
    ///
    /// Returns an error when the note cannot be saved.
    pub fn note_update_asked(&self, chat_id: &str) -> Result<(), Error> {
        let note = AfterUpdate {
            from: env!("CARGO_PKG_VERSION").to_string(),
            conversations: self.bound_conversations(chat_id),
        };
        let json = serde_json::to_string(&note).map_err(|e| e.to_string())?;
        self.store.set_state(AFTER_UPDATE, &json)
    }
}
