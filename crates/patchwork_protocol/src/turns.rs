//! Driving a chat's turn: sending to it and answering what it asks.

// Rust guideline compliant 2026-10-09

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The params of `turn.send`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendParams {
    pub chat_id: String,
    pub text: String,
}

/// The answer to `turn.send`: the message made, and how it went in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sent {
    pub message_id: String,
    /// It went into the running turn.
    pub steered: bool,
    /// It waits for the running turn to end.
    pub queued: bool,
}

/// The params of `approval.answer`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerParams {
    pub chat_id: String,
    pub approval_id: String,
    /// Allows it; left out or null denies.
    #[serde(default)]
    pub allow: Option<bool>,
    /// Answers to its questions, by question id.
    #[serde(default)]
    pub answers: Option<BTreeMap<String, Vec<String>>>,
}
