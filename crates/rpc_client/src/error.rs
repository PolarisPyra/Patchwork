//! Errors returned by [`crate::RpcClient`] and [`crate::WsClient`].

// Rust guideline compliant 2026-10-09

use thiserror::Error;

/// Errors that can occur when talking to a Patchwork RPC server.
#[derive(Debug, Error)]
pub enum ClientError {
    /// The server URL is malformed or uses an unsupported scheme.
    #[error("invalid server url: {0}")]
    InvalidUrl(String),

    /// Transport-level HTTP failure.
    #[error("http request failed: {0}")]
    Http(#[source] Box<ureq::Error>),

    /// Server returned an unexpected non-200 HTTP status.
    #[error("http status {status}: {body}")]
    HttpStatus { status: u16, body: String },

    /// Transport-level WebSocket failure.
    #[error("websocket error: {0}")]
    WebSocket(#[source] Box<tungstenite::Error>),

    /// Underlying socket I/O error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Encoding or decoding JSON failed.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// Authentication failed (HTTP 401 or WebSocket close code 4401).
    #[error("unauthorized: {0}")]
    Unauthorized(String),

    /// The server returned an RPC-level error frame.
    #[error("rpc error: {0}")]
    Rpc(String),

    /// The server returned a frame that did not match the expected RPC or push schema.
    #[error("unexpected frame: {0}")]
    UnexpectedFrame(String),

    /// The WebSocket connection was closed by the server.
    #[error("websocket closed (code: {0:?})")]
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
