//! The open chat's messages.

// Rust guideline compliant 2026-10-09

use patchwork_protocol::messages::Message;

/// The open chat's messages, oldest first.
#[derive(Debug, Clone, Default)]
pub struct MessagesStore {
    /// The messages, oldest first.
    pub list: Vec<Message>,
}

impl MessagesStore {
    /// Keeps `message` in place of the one with its id, or last when it is new.
    pub fn upsert(&mut self, message: Message) {
        match self.list.iter_mut().find(|m| m.id == message.id) {
            Some(kept) => *kept = message,
            None => self.list.push(message),
        }
    }

    /// Drops message `id`.
    pub fn remove(&mut self, id: &str) {
        self.list.retain(|m| m.id != id);
    }

    /// Drops every message.
    pub fn clear(&mut self) {
        self.list.clear();
    }
}
