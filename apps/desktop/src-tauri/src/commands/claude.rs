//! The Claude Code CLI: its version and commands.

use super::*;

/// What the installed Claude Code offers: its path and version.
#[tauri::command]
pub(crate) async fn claude_info() -> Res<ClaudeInfo> {
    blocking(patchwork_server::provider::claude_cli::info).await?
}

/// The output of one of the Claude Code CLI's read-only diagnostics commands.
#[tauri::command]
pub(crate) async fn claude_command(kind: String) -> Res<String> {
    let kind = match kind.as_str() {
        "doctor" => patchwork_server::provider::cli_info::InfoCommand::Doctor,
        "mcp" => patchwork_server::provider::cli_info::InfoCommand::McpList,
        "plugins" => patchwork_server::provider::cli_info::InfoCommand::PluginList,
        other => return Err(format!("There is no Claude command named {other}")),
    };
    blocking(move || patchwork_server::provider::claude_cli::command(kind)).await?
}
