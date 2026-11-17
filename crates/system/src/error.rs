//! The error the backend returns. It serializes to its message, so the frontend shows the same
//! text it did when errors were plain strings.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use serde::{Serialize, Serializer};

/// What went wrong: a kind callers can tell apart, and a message to show the user.
///
/// It is cheap to clone, so a failure can be kept and shown again, as the MCP servers that failed
/// to start are.
#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    /// A path that resolves outside the project it was given for.
    #[error("Path is outside the project")]
    OutsideProject,
    /// A git command that failed, with what it was asked to do and what it said.
    #[error("{0}")]
    Git(String),
    /// A file-system or process error from the standard library.
    #[error(transparent)]
    Io(Arc<std::io::Error>),
    /// A tool that failed: what went wrong and what to do about it, and what the tool printed.
    #[error("{message}")]
    Tool {
        message: String,
        /// The tool's own output, shown when the person asks for it.
        detail: String,
    },
    /// Any other failure, with the message to show.
    #[error("{0}")]
    Message(String),
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(Arc::new(error))
    }
}

impl From<String> for Error {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}

impl From<&str> for Error {
    fn from(message: &str) -> Self {
        Self::Message(message.to_string())
    }
}

impl Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}
