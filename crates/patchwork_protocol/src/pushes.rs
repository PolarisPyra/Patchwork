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

    /// Reads a push from its name and `data`, as a client gets them in a [`PushFrame`].
    ///
    /// Returns `None` for a push this build does not know, which a client skips.
    ///
    /// # Errors
    ///
    /// Returns the deserializer's message when `data` is not the named push's.
    ///
    /// [`PushFrame`]: crate::frames::PushFrame
    pub fn from_frame(
        name: &str,
        data: serde_json::Value,
    ) -> Result<Option<Self>, serde_json::Error> {
        Ok(Some(match name {
            "chat.upserted" => Self::ChatUpserted(serde_json::from_value(data)?),
            "message.upserted" => Self::MessageUpserted(serde_json::from_value(data)?),
            "message.deleted" => Self::MessageDeleted(serde_json::from_value(data)?),
            "chat.deleted" => Self::ChatDeleted(serde_json::from_value(data)?),
            _ => return Ok(None),
        }))
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
