//! Launch arguments for Claude Code turns.

use crate::types::Settings;

/// Arguments that append the shell instructions to Claude's system prompt, and load
/// Patchwork's skills as a plugin.
pub fn launch_args(settings: &Settings) -> Vec<String> {
    let mut args = vec![
        "--append-system-prompt".to_string(),
        super::shell_instructions(settings),
    ];
    if let Some(plugin) = crate::skills::claude_plugin(settings) {
        args.extend(["--plugin-dir".to_string(), plugin.to_string_lossy().into_owned()]);
    }
    args
}
