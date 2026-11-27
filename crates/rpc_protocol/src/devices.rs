//! Pairing and paired devices, and the event log, which the server answers itself.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};

use crate::frames::PushFrame;

/// A paired device, as the web client's `Device` type has it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    /// What it runs, such as `ios`, `android` or `web`.
    pub platform: String,
    /// What its token may do: `read`, `write`, `terminal`, `admin`.
    pub scopes: Vec<String>,
    pub created_at: i64,
    pub last_seen_at: Option<i64>,
    /// The token's first few characters.
    pub token_hint: String,
}

/// The answer to `auth.pair.start`: a code a device can claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingCode {
    pub code: String,
    pub expires_at: i64,
    pub scopes: Vec<String>,
}

/// The answer to `auth.pair.claim`: the device's token, shown once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Claimed {
    pub token: String,
    pub device: Device,
}

/// The answer to `pairing.info`: where devices reach the app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingInfo {
    pub urls: Vec<String>,
    /// The app's own token for an admin, else empty.
    pub token: String,
    /// Whether devices on the network can reach the app.
    pub reachable: bool,
}

/// The answer to `events.since`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventsSince {
    /// The oldest push that could be replayed.
    pub floor: u64,
    /// The sequence number of the next push.
    pub next: u64,
    /// Pushes to replay; the app keeps none, so a client that missed any reloads.
    pub events: Vec<PushFrame>,
}
