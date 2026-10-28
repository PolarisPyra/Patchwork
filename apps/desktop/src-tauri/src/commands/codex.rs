//! The Codex CLI: its version, features, and commands.

use super::*;

/// What the installed Codex CLI offers: its version, flags, and models.
#[tauri::command]
pub(crate) async fn codex_info() -> Res<CodexInfo> {
    blocking(patchwork_server::provider::codex_cli::info).await?
}

/// The output of one of the Codex CLI's read-only diagnostics commands.
#[tauri::command]
pub(crate) async fn codex_command(kind: String) -> Res<String> {
    let kind = match kind.as_str() {
        "doctor" => patchwork_server::provider::cli_info::InfoCommand::Doctor,
        "mcp" => patchwork_server::provider::cli_info::InfoCommand::McpList,
        "plugins" => patchwork_server::provider::cli_info::InfoCommand::PluginList,
        other => return Err(format!("There is no Codex command named {other}")),
    };
    blocking(move || patchwork_server::provider::codex_cli::command(kind)).await?
}
