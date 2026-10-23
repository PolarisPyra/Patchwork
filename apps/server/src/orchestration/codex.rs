//! Instructions for Codex turns. Codex takes them in its `thread/start` and `thread/resume`
//! requests as `developerInstructions`, which it adds to its own.

use crate::types::Settings;

/// The developer instructions Codex runs with: the shell instructions. None when there are none.
pub fn developer_instructions(settings: &Settings) -> Option<String> {
    Some(super::shell_instructions(settings)).filter(|text| !text.is_empty())
}
