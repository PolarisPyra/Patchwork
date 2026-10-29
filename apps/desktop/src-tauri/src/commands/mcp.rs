//! MCP servers: saving, removing, and enabling them for the CLIs.

use super::*;

#[tauri::command]
pub(crate) fn list_mcp_servers() -> Vec<provider::mcp::ServerView> {
    provider::mcp::list()
}

#[tauri::command]
pub(crate) async fn save_mcp_server(
    name: String,
    previous: Option<String>,
    server: provider::mcp::McpServer,
) -> Res<()> {
    blocking(move || provider::mcp::save(name, previous, server)).await?
}

#[tauri::command]
pub(crate) async fn remove_mcp_server(name: String) -> Res<()> {
    blocking(move || provider::mcp::remove(&name)).await?
}

#[tauri::command]
pub(crate) async fn restart_mcp_server(name: String) -> Res<()> {
    blocking(move || provider::mcp::restart(&name)).await
}

#[tauri::command]
pub(crate) fn mcp_config_file() -> String {
    provider::mcp::path().display().to_string()
}

/// How many MCP tools an API turn may offer.
#[tauri::command]
pub(crate) fn mcp_tool_budget() -> usize {
    provider::mcp::tool_budget()
}

/// The MCP servers Claude Code and Codex use, from their own config.
#[tauri::command]
pub(crate) async fn list_cli_mcp_servers(
    server: Srv<'_>,
) -> Res<Vec<patchwork_server::provider::mcp::clis::CliGroup>> {
    let projects = server.store.settings().projects;
    blocking(move || patchwork_server::provider::mcp::clis::list(&projects)).await
}

#[tauri::command]
pub(crate) async fn refresh_cli_mcp_server(server: Srv<'_>, key: String) -> Res<()> {
    let projects = server.store.settings().projects;
    blocking(move || patchwork_server::provider::mcp::clis::refresh(&key, &projects)).await
}

#[tauri::command]
pub(crate) async fn enable_cli_mcp_server(server: Srv<'_>, key: String) -> Res<()> {
    let projects = server.store.settings().projects;
    blocking(move || patchwork_server::provider::mcp::clis::enable(&key, &projects)).await?
}

/// Opens a file MCP servers are configured in, in the user's editor.
#[tauri::command]
pub(crate) async fn open_mcp_config(app: AppHandle, server: Srv<'_>, path: String) -> Res<()> {
    let path = std::path::PathBuf::from(path);
    if !patchwork_server::provider::mcp::clis::is_config_file(
        &path,
        &server.store.settings().projects,
    ) {
        return Err("That is not an MCP config file".into());
    }
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Err("That is not an MCP config file".into());
    };
    edit_file(
        &app,
        dir.to_string_lossy().into(),
        name.to_string_lossy().into(),
    )
    .await
}
