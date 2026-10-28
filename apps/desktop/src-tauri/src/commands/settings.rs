//! Settings, projects, skills, and custom agents, and the folders they live in.

use super::*;

#[tauri::command]
pub(crate) fn get_settings(server: Srv) -> Settings {
    server.store.settings()
}

/// The rules Settings → Permissions starts with, which Restore defaults puts back.
#[tauri::command]
pub(crate) async fn default_permission_rules() -> Res<Vec<patchwork_server::types::PermissionRule>>
{
    Ok(patchwork_server::permissions::default_rules())
}

#[tauri::command]
pub(crate) fn save_settings(app: AppHandle, server: Srv, settings: Settings) -> Res<()> {
    let logo = settings.logo.clone();
    let logo_changed = server.store.settings().logo != logo;
    server.save_settings(settings)?;
    if logo_changed {
        logo::apply(&app, logo.as_deref());
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn list_shells() -> ShellChoices {
    ShellChoices {
        shells: patchwork_server::available_shells(),
        default: patchwork_server::effective_shell(None),
    }
}

#[derive(serde::Serialize)]
pub(crate) struct SkillsView {
    dir: String,
    skills: Vec<patchwork_server::skills::Skill>,
}

/// The skills folder in use and the skills in it.
#[tauri::command]
pub(crate) fn list_skills(server: Srv) -> SkillsView {
    let dir = patchwork_server::skills::dir(&server.store.settings());
    SkillsView {
        skills: patchwork_server::skills::all(&server.store.settings()),
        dir: dir.to_string_lossy().into_owned(),
    }
}

/// The skills, as `list_skills` gives them, and then again on `on_change` after each change to
/// the skills folder, until `unwatch_skills` or another watch. Not `async`, so a watch and the
/// unwatch before it run in the order they were called.
#[tauri::command]
pub(crate) fn watch_skills(server: Srv, on_change: Channel<SkillsView>) -> Res<SkillsView> {
    let view = list_skills(server.clone());
    let dir = view.dir.clone();
    server.watch_skills(move |skills| {
        let _ = on_change.send(SkillsView {
            dir: dir.clone(),
            skills,
        });
    })?;
    Ok(view)
}

#[tauri::command]
pub(crate) fn unwatch_skills(server: Srv) {
    server.unwatch_skills();
}

/// Opens a skill's file in the user's editor.
#[tauri::command]
pub(crate) async fn open_skill(app: AppHandle, server: Srv<'_>, path: String) -> Res<()> {
    if !patchwork_server::skills::all(&server.store.settings())
        .iter()
        .any(|s| s.path == path)
    {
        return Err("That is not a skill".into());
    }
    let path = std::path::PathBuf::from(path);
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Err("That is not a skill".into());
    };
    edit_file(
        &app,
        dir.to_string_lossy().into(),
        name.to_string_lossy().into(),
    )
    .await
}

/// Opens the skills folder in the file manager, creating it first.
#[tauri::command]
pub(crate) async fn open_skills_folder(app: AppHandle, server: Srv<'_>) -> Res<()> {
    open_folder(
        &app,
        &patchwork_server::skills::dir(&server.store.settings()),
    )
    .await
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

/// Opens the folder of the debug logs in the file manager, creating it first.
#[tauri::command]
pub(crate) async fn open_logs_folder(app: AppHandle) -> Res<()> {
    let dir = app
        .path()
        .config_dir()
        .map_err(|e| e.to_string())?
        .join("patchwork/logs");
    open_folder(&app, &dir).await
}

/// This launch's logs, for the Output panel.
#[tauri::command]
pub(crate) fn log_channels() -> Vec<String> {
    patchwork_server::debug_log::channels()
}

/// A log from `offset` on, for the Output panel to follow.
#[tauri::command]
pub(crate) async fn read_log(
    channel: String,
    offset: u64,
) -> Res<patchwork_server::debug_log::LogChunk> {
    blocking(move || patchwork_server::debug_log::read(&channel, offset)).await?
}

/// A message from the window for `window/renderer.log`.
#[tauri::command]
pub(crate) fn log_renderer(level: patchwork_server::types::LogLevel, message: String) {
    use patchwork_server::debug_log::{Data, RENDERER, Source};
    RENDERER.write(level, "", &Source::default(), Data::Text(message));
}

/// Saves the kept sessions' logs as a zip at `path`, with `system.json`: the app's version,
/// the platform, the connected agents' CLI versions, and the settings, secrets removed.
#[tauri::command]
pub(crate) async fn export_logs(app: AppHandle, server: Srv<'_>, path: String) -> Res<()> {
    let server = server.inner().clone();
    let version = app.package_info().version.to_string();
    blocking(move || {
        let settings = server.store.settings();
        let agents: serde_json::Map<String, serde_json::Value> = settings
            .agents
            .iter()
            .map(|&kind| {
                let version = provider::cli_version(kind);
                (format!("{kind:?}"), serde_json::json!(version))
            })
            .collect();
        let system = serde_json::json!({
            "app": "Patchwork",
            "version": version,
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "agents": agents,
            "settings": settings,
        });
        patchwork_server::debug_log::export(std::path::Path::new(&path), &system)
    })
    .await?
}
