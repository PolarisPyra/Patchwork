//! What goes wrong calling a Patchwork.

// Rust guideline compliant 2026-10-09

use thiserror::Error;

/// Why a call to a Patchwork failed.
#[derive(Debug, Error)]
pub enum ClientError {
    /// The address is malformed or not `http`, `https`, `ws` or `wss`.
    #[error("invalid server url: {0}")]
    InvalidUrl(String),

    /// An HTTP request failed on its way.
    #[error("http request failed: {0}")]
    Http(#[source] Box<ureq::Error>),

    /// The server answered an HTTP request with a status other than 200.
    #[error("http status {status}: {body}")]
    HttpStatus {
        /// The status code.
        status: u16,
        /// The answer's body.
        body: String,
    },

    /// The socket failed.
    #[error("websocket error: {0}")]
    WebSocket(#[source] Box<tungstenite::Error>),

    /// Reading or writing the connection failed.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// A frame's JSON did not read or write.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// The token was refused (HTTP 401 or socket close code 4401).
    #[error("unauthorized: {0}")]
    Unauthorized(String),

    /// The Patchwork answered with an error, or its answer was not the method's.
    #[error("{0}")]
    Rpc(String),

    /// A frame was neither an answer nor a push.
    #[error("unexpected frame: {0}")]
    UnexpectedFrame(String),

    /// The connection closed, with the close code when the server gave one.
    #[error("connection closed (code: {0:?})")]
    Closed(Option<u16>),
}

impl From<ureq::Error> for ClientError {
    fn from(error: ureq::Error) -> Self {
        Self::Http(Box::new(error))
    }
}

impl From<tungstenite::Error> for ClientError {
    fn from(error: tungstenite::Error) -> Self {
        Self::WebSocket(Box::new(error))
    }
}
