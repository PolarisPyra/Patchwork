//! Embedded HTTP and WebSocket RPC server for Patchwork.
//!
//! Clients speak JSON requests over `POST /rpc` or a WebSocket at `/ws`, and receive push frames
//! over the socket as chats change. Every connection carries a token: the server's own token
//! (generated on start) or one a paired device received via pairing, restricted to its scopes.

// Rust guideline compliant 2026-10-09

mod http;
mod listener;
pub mod memory;
pub mod pairing;
mod rpc;
pub mod scopes;
#[cfg(test)]
mod tests;

pub use listener::{DEFAULT_PORT, Options, Server};
pub use memory::MemoryPairingStore;
pub use rpc::{Backend, CodeGrant, PairingStore, Touched};

/// Status and connection details for a running RPC server.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerInfo {
    /// The loopback address this machine reaches the server at, such as `http://127.0.0.1:7703`.
    pub address: String,
    /// The addresses other devices on the network reach it at; empty while network access is off.
    pub urls: Vec<String>,
    /// The token every admin connection must carry.
    pub token: String,
    /// Whether devices on the network may connect (the saved setting).
    pub network: bool,
    /// Whether the saved network setting differs from what the running server listens on.
    pub restart_needed: bool,
    /// Why the server is not running, when it is not.
    pub error: Option<String>,
}

/// A pairing code a new device types or scans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairCode {
    /// The code, such as `K7M-QX2`.
    pub code: String,
    /// When it stops working, in milliseconds since the Unix epoch.
    pub expires_at_ms: i64,
    /// The link that pairs a device in one step, such as `http://192.168.1.5:7703/pair?code=K7M-QX2`.
    pub link: String,
}

/// A device paired with this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    /// What the device said it runs, such as `ios`, `android` or `web`.
    pub platform: String,
    /// What its token may do: `read`, `write`, `terminal`, `admin`.
    pub scopes: Vec<String>,
    pub created_at_ms: i64,
    /// When it last connected, when it has.
    pub last_seen_ms: Option<i64>,
}

/// A paired device as stored: with a short hint of its token so two devices with the same name
/// can be told apart. The token itself is never stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairedDevice {
    pub device: Device,
    /// The token's first few characters.
    pub token_hint: String,
}

/// Current time in milliseconds since the Unix epoch, as device tables store it.
#[must_use]
pub fn now_ms() -> i64 {
    i64::try_from(patchwork_platform::now_ms()).unwrap_or(i64::MAX)
}

/// The QR code for `text`, as rows of dark (`true`) and light modules, without the quiet zone.
/// Returns `None` when the text is too long for a QR code.
#[must_use]
pub fn qr_modules(text: &str) -> Option<Vec<Vec<bool>>> {
    // Medium correction survives a smudged or glary screen and still fits a pairing link in a
    // small code.
    let qr = qrcodegen::QrCode::encode_text(text, qrcodegen::QrCodeEcc::Medium).ok()?;
    let size = qr.size();
    Some(
        (0..size)
            .map(|y| (0..size).map(|x| qr.get_module(x, y)).collect())
            .collect(),
    )
}
