//! What an agent's request for a secret carries, which the chat shows as a private card.

// Rust guideline compliant 2026-11-27

use serde::{Deserialize, Serialize};

/// Where a secret request stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SecretStatus {
    Pending,
    Saved,
    Declined,
    /// The turn ended, or the agent stopped waiting, before the user answered.
    Cancelled,
    TimedOut,
}

/// What makes an approval a request for a secret, whose title is the card's label and whose
/// detail says what it is for. It never holds the value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Secret {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    pub status: SecretStatus,
}
