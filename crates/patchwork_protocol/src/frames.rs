//! The frames around every answer and push, over HTTP and over a socket.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A request's answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Answer {
    /// The request's id, or null when it had none.
    pub id: Value,
    pub result: Value,
}

/// A request that failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    /// The request's id where it is known, else null.
    pub id: Value,
    pub error: ErrorMessage,
}

/// A request over HTTP turned away before it was read, as for a missing token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rejected {
    pub error: ErrorMessage,
}

/// What went wrong, as the client shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorMessage {
    pub message: String,
}

impl ErrorMessage {
    /// An error saying `message`.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// A change sent to open sockets, numbered so a client can tell it missed one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushFrame {
    /// The push's name, such as `chat.upserted`.
    pub push: String,
    pub data: Value,
    pub seq: u64,
}

/// The answer to `GET /health`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Health {
    pub ok: bool,
    pub version: String,
    /// The sequence number of the next push.
    pub seq: u64,
}
