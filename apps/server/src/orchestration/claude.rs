//! Launch arguments for Claude Code turns.

use crate::types::Settings;

/// Arguments that append the shell instructions to Claude's system prompt.
pub fn launch_args(settings: &Settings) -> Vec<String> {
    vec![
        "--append-system-prompt".to_string(),
        super::shell_instructions(settings),
    ]
}
