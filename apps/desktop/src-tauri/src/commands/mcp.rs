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

/// How many MCP tools an API turn may offer.
#[tauri::command]
pub(crate) fn mcp_tool_budget() -> usize {
    provider::mcp::tool_budget()
}

#[tauri::command]
pub(crate) fn mcp_config_file() -> String {
    provider::mcp::path().display().to_string()
}

/// The MCP servers Claude Code and Codex use, from their own config.
#[tauri::command]
pub(crate) async fn list_cli_mcp_servers(
    server: Srv<'_>,
) -> Res<Vec<provider::mcp_clis::CliGroup>> {
    let projects = server.store.settings().projects;
    blocking(move || provider::mcp_clis::list(&projects)).await
}

#[tauri::command]
pub(crate) async fn refresh_cli_mcp_server(server: Srv<'_>, key: String) -> Res<()> {
    let projects = server.store.settings().projects;
    blocking(move || provider::mcp_clis::refresh(&key, &projects)).await
}

#[tauri::command]
pub(crate) async fn enable_cli_mcp_server(server: Srv<'_>, key: String) -> Res<()> {
    let projects = server.store.settings().projects;
    blocking(move || provider::mcp_clis::enable(&key, &projects)).await?
}

/// Opens a file MCP servers are configured in, in the user's editor.
#[tauri::command]
pub(crate) async fn open_mcp_config(app: AppHandle, server: Srv<'_>, path: String) -> Res<()> {
    let path = std::path::PathBuf::from(path);
    if !provider::mcp_clis::is_config_file(&path, &server.store.settings().projects) {
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

#[tauri::command]
pub(crate) async fn fetch_api_models(
    query: provider::api_config::ModelsQuery,
) -> Res<Vec<provider::api_config::ApiModel>> {
    blocking(move || provider::api_config::fetch_models(query)).await?
}

#[tauri::command]
pub(crate) async fn detect_agent(kind: AgentKind) -> AgentStatus {
    blocking(move || provider::detect_agent(kind))
        .await
        .unwrap_or_else(|e| AgentStatus {
            kind,
            installed: false,
            version: None,
            signed_in: false,
            detail: e,
        })
}

#[tauri::command]
/// Cached; `refresh` asks for new numbers, which the server still throttles.
pub(crate) async fn get_usage(
    server: Srv<'_>,
    kind: AgentKind,
    refresh: bool,
) -> Res<ProviderUsage> {
    let server = server.inner().clone();
    blocking(move || server.usage.get(kind, refresh)).await
}

#[tauri::command]
pub(crate) fn add_project(server: Srv, path: String) -> Res<Project> {
    server.add_project(&path)
}

#[tauri::command]
pub(crate) fn create_project(server: Srv, parent: String, name: String) -> Res<Project> {
    server.create_project(&parent, &name)
}

#[tauri::command]
pub(crate) fn rename_project(server: Srv, id: String, name: String) -> Res<()> {
    server.rename_project(&id, &name)
}

#[tauri::command]
pub(crate) fn relocate_project(server: Srv, id: String, path: String) -> Res<()> {
    server.relocate_project(&id, &path)
}

#[tauri::command]
pub(crate) fn is_folder(path: String) -> bool {
    std::path::Path::new(&path).is_dir()
}

#[tauri::command]
pub(crate) fn rename_thread(server: Srv, id: String, title: String) -> Res<()> {
    server.rename_thread(&id, &title)
}

#[tauri::command]
pub(crate) fn remove_project(server: Srv, id: String) -> Res<()> {
    server.remove_project(&id)
}
