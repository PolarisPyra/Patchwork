//! Typed HTTP and WebSocket client runtime for Patchwork RPC servers.
//!
//! Built directly on `patchwork-rpc-protocol`, this crate provides:
//! - [`RpcClient`] for `GET /health`, `POST /rpc`, pairing, and device management.
//! - [`WsClient`] for bidirectional `/ws` RPC calls and live [`patchwork_rpc_protocol::frames::PushFrame`]
//!   streaming.

// Rust guideline compliant 2026-10-09

mod error;
mod http;
mod ws;

pub use error::ClientError;
pub use http::RpcClient;
pub use ws::WsClient;
