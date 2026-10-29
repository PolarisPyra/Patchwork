//! The agent CLIs: finding and installing them, what Claude Code and Codex report about themselves, and the custom agents a chat can summon.

use super::*;

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
pub(crate) async fn install_cli(server: Srv<'_>, kind: AgentKind) -> Res<()> {
    let server = server.inner().clone();
    tauri::async_runtime::spawn_blocking(move || server.install_cli(kind))
        .await
        .map_err(|e| e.to_string())?
}

/// What the installed Claude Code offers: its path and version.
#[tauri::command]
pub(crate) async fn claude_info() -> Res<ClaudeInfo> {
    blocking(patchwork_server::provider::claude::cli::info).await?
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
    blocking(move || patchwork_server::provider::claude::cli::command(kind)).await?
}

/// What the installed Codex CLI offers: its version, flags, and models.
#[tauri::command]
pub(crate) async fn codex_info() -> Res<CodexInfo> {
    blocking(patchwork_server::provider::codex::cli::info).await?
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
    blocking(move || patchwork_server::provider::codex::cli::command(kind)).await?
}

#[derive(serde::Serialize)]
pub(crate) struct AgentsView {
    dir: String,
    agents: std::sync::Arc<[patchwork_server::agents::Agent]>,
}

/// The custom agents and their folder now, and again on `on_change` after each change to them,
/// until another watch takes its place.
#[tauri::command]
pub(crate) fn watch_agents(server: Srv, on_change: Channel<AgentsView>) -> AgentsView {
    let dir = server.agents.dir().to_string_lossy().into_owned();
    let sent = dir.clone();
    server.agents.subscribe(move |agents| {
        let _ = on_change.send(AgentsView {
            dir: sent.clone(),
            agents,
        });
    });
    AgentsView {
        dir,
        agents: server.agents.list(),
    }
}

/// Opens a custom agent's file in the user's editor.
#[tauri::command]
pub(crate) async fn open_agent(app: AppHandle, server: Srv<'_>, path: String) -> Res<()> {
    if !server.agents.list().iter().any(|a| a.path == path) {
        return Err("That is not an agent".into());
    }
    let path = std::path::PathBuf::from(path);
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Err("That is not an agent".into());
    };
    edit_file(
        &app,
        dir.to_string_lossy().into(),
        name.to_string_lossy().into(),
    )
    .await
}

/// The custom agents a chat in the project can summon, with the project's own.
#[tauri::command]
pub(crate) async fn list_agents(
    server: Srv<'_>,
    project_id: Option<String>,
) -> Res<Vec<patchwork_server::agents::Agent>> {
    let server = server.inner().clone();
    blocking(move || server.project_agents(project_id.as_deref())).await
}

/// Adds an agent to the agents folder and opens it in the user's editor.
#[tauri::command]
pub(crate) async fn create_agent(app: AppHandle, server: Srv<'_>, name: String) -> Res<()> {
    let server = server.inner().clone();
    let dir = server.agents.dir().to_string_lossy().into_owned();
    let file = format!("{name}.md");
    blocking(move || server.agents.create(&name)).await??;
    edit_file(&app, dir, file).await
}
