//! Instructions for Codex turns. Codex takes them in its `thread/start` and `thread/resume`
//! requests as `developerInstructions`, which it adds to its own.

use crate::types::{AgentKind, Settings};

/// The developer instructions Codex runs with: its checklist and shell instructions. None when
/// there are none.
pub fn developer_instructions(settings: &Settings) -> Option<String> {
    Some(super::agent_instructions(settings, AgentKind::Codex)).filter(|text| !text.is_empty())
}
