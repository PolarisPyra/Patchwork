//! The error the backend returns. It serializes to its message, so the frontend shows the same
//! text it did when errors were plain strings.

use serde::{Serialize, Serializer};

/// What went wrong: a kind callers can tell apart, and a message to show the user.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A path that resolves outside the project it was given for.
    #[error("Path is outside the project")]
    OutsideProject,
    /// A git command that failed, with what it was asked to do and what it said.
    #[error("{0}")]
    Git(String),
    /// A file-system or process error from the standard library.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Any other failure, with the message to show.
    #[error("{0}")]
    Message(String),
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

/// Lets the modules that still pass plain strings take a typed error with `?`, and read its message.
impl From<Error> for String {
    fn from(error: Error) -> Self {
        error.to_string()
    }
}
