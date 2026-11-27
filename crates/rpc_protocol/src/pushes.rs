//! What the server pushes to open sockets when a chat changes.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};

use crate::chats::Chat;
use crate::messages::Message;

/// One change a client is told about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Push {
    /// `chat.upserted`: a chat was made or changed.
    ChatUpserted(Chat),
    /// `message.upserted`: a message was added or changed.
    MessageUpserted(Message),
    /// `message.deleted`: a message left its chat.
    MessageDeleted(MessageGone),
    /// `chat.deleted`: a chat was deleted.
    ChatDeleted(ChatGone),
}

impl Push {
    /// The push's name, as a frame's `push` field has it.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::ChatUpserted(_) => "chat.upserted",
            Self::MessageUpserted(_) => "message.upserted",
            Self::MessageDeleted(_) => "message.deleted",
            Self::ChatDeleted(_) => "chat.deleted",
        }
    }

    /// The push's `data`.
    ///
    /// # Errors
    ///
    /// Returns the serializer's message; these types always serialize.
    pub fn data(&self) -> Result<serde_json::Value, serde_json::Error> {
        match self {
            Self::ChatUpserted(chat) => serde_json::to_value(chat),
            Self::MessageUpserted(message) => serde_json::to_value(message),
            Self::MessageDeleted(gone) => serde_json::to_value(gone),
            Self::ChatDeleted(gone) => serde_json::to_value(gone),
        }
    }
}

/// The data of `message.deleted`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageGone {
    pub chat_id: String,
    pub id: String,
}

/// The data of `chat.deleted`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatGone {
    pub id: String,
}

#[cfg(test)]
mod tests {
    use super::{ChatGone, MessageGone, Push};

    #[test]
    fn deletions_have_the_client_shape() {
        let message = Push::MessageDeleted(MessageGone {
            chat_id: "c-1".into(),
            id: "m-1".into(),
        });
        assert_eq!(message.name(), "message.deleted");
        assert_eq!(
            message.data().expect("JSON").to_string(),
            r#"{"chatId":"c-1","id":"m-1"}"#
        );
        let chat = Push::ChatDeleted(ChatGone { id: "c-1".into() });
        assert_eq!(chat.name(), "chat.deleted");
        assert_eq!(chat.data().expect("JSON").to_string(), r#"{"id":"c-1"}"#);
    }
}
