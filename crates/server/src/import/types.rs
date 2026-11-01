//! Conversations other apps saved, offered for import.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Where imported conversations come from: an agent CLI's own session
/// files, or another app (OpenCode, for one) that ran those agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "import.ts")]
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
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "import.ts")]
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
