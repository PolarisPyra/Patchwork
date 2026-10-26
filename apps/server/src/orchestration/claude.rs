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

/// Environment for Claude's process. The claude.ai connectors are left out, since their tool
/// lists and instructions are added to the context again whenever a session resumes. A set
/// compact window makes Claude compact once its context reaches it.
pub fn launch_env(settings: &Settings) -> Vec<(&'static str, String)> {
    let mut env = vec![("ENABLE_CLAUDEAI_MCP_SERVERS", "false".to_string())];
    let window = settings.tokens.claude_compact_window;
    if window > 0 {
        env.push(("CLAUDE_CODE_AUTO_COMPACT_WINDOW", window.to_string()));
    }
    env
}
