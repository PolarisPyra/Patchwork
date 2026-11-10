//! Conversations other apps saved, offered for import.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};

/// Where imported conversations come from: an agent CLI's own session
/// files, or another app (OpenCode, for one) that ran those agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportSource {
    Claude,
    Codex,
    #[serde(alias = "gemini")]
    Antigravity,
    T3,
    OpenCode,
}

/// Conversations a source has saved on disk that can be imported.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportScan {
    pub kind: ImportSource,
    /// Sessions found on disk.
    pub found: u32,
    /// Sessions not yet in Patchwork.
    pub new: u32,
    /// Helper runs inside other conversations; not offered for import.
    pub subagents: u32,
}
