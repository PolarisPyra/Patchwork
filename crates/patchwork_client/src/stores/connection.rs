//! How far a client is with reaching a Patchwork.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use crate::transport::Transport;

/// How far the client is with reaching a Patchwork.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Pairing {
    /// Not paired yet.
    #[default]
    Unpaired,
    /// Claiming a code and connecting.
    Connecting,
    /// Connected to the Patchwork at this address.
    Connected(String),
    /// Pairing or connecting failed, for this reason.
    Failed(String),
}

/// The transport a client calls through, how pairing stands, and the last call that failed.
#[derive(Default)]
pub struct ConnectionState {
    pub(crate) transport: Option<Arc<dyn Transport>>,
    /// How pairing stands.
    pub pairing: Pairing,
    /// The last call that failed, for a screen to show.
    pub error: Option<String>,
}

impl std::fmt::Debug for ConnectionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionState")
            .field("connected", &self.transport.is_some())
            .field("pairing", &self.pairing)
            .field("error", &self.error)
            .finish()
    }
}

impl ConnectionState {
    /// Whether the client has a transport to call through.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.transport.is_some()
    }
}
