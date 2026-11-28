//! The chats a client lists, and which one is open.

// Rust guideline compliant 2026-10-09

use patchwork_protocol::chats::Chat;

/// Every chat a person started, in the Patchwork's order, and the one whose messages are loaded.
#[derive(Debug, Clone, Default)]
pub struct ChatsStore {
    /// Every chat, in the Patchwork's order.
    pub list: Vec<Chat>,
    /// The chat whose messages are loaded.
    pub open: Option<String>,
}

impl ChatsStore {
    /// The open chat, when there is one.
    #[must_use]
    pub fn open_chat(&self) -> Option<&Chat> {
        let id = self.open.as_deref()?;
        self.find(id)
    }

    /// Chat `id`.
    #[must_use]
    pub fn find(&self, id: &str) -> Option<&Chat> {
        self.list.iter().find(|c| c.id == id)
    }

    /// Whether chat `id` is the open one.
    #[must_use]
    pub fn is_open(&self, id: &str) -> bool {
        self.open.as_deref() == Some(id)
    }

    /// Keeps `chat` in place of the one with its id, or first when it is new.
    pub fn upsert(&mut self, chat: Chat) {
        match self.list.iter_mut().find(|c| c.id == chat.id) {
            Some(kept) => *kept = chat,
            None => self.list.insert(0, chat),
        }
    }

    /// Drops chat `id`, and closes it when it is open. Returns whether it was open.
    pub fn remove(&mut self, id: &str) -> bool {
        self.list.retain(|c| c.id != id);
        let was_open = self.is_open(id);
        if was_open {
            self.open = None;
        }
        was_open
    }
}
