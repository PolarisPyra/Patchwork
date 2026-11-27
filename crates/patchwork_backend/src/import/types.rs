//! Conversations other apps saved, offered for import.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};

use crate::provider::AgentKind;

/// Where imported conversations come from: an agent CLI's own session
/// files, or another app (OpenCode, for one) that ran those agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportSource {
    Claude,
    Codex,
    Antigravity,
    T3,
    OpenCode,
}

impl ImportSource {
    /// The agent whose name and logo the source shows; T3 has none.
    #[must_use]
    pub fn agent(self) -> Option<AgentKind> {
        match self {
            Self::Claude => Some(AgentKind::Claude),
            Self::Codex => Some(AgentKind::Codex),
            Self::Antigravity => Some(AgentKind::Antigravity),
            Self::OpenCode => Some(AgentKind::OpenCode),
            Self::T3 => None,
        }
    }

    /// The sources offered for the connected `agents`: each one's own files, then OpenCode's.
    #[must_use]
    pub fn offered(agents: &[AgentKind]) -> Vec<Self> {
        [Self::Claude, Self::Codex, Self::Antigravity]
            .into_iter()
            .filter(|source| source.agent().is_some_and(|agent| agents.contains(&agent)))
            .chain([Self::OpenCode])
            .collect()
    }
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
