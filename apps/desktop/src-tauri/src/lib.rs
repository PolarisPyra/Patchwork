//! Tauri shell: exposes `patchwork_server::Server` as commands and forwards
//! agent events to the web UI on the `"agent-event"` channel.

mod clipboard;
mod logo;
mod terminal;
mod updates;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};

use patchwork_server::types::{AgentKind, Project, Settings, AgentStatus, ModelOption, ShellChoices, Role, MessageStatus, AccessMode, Attachment, Message, Thread, Folder, GoalStatus, FileChange, ProviderUsage, ImportSource, ImportScan, AgentEventEnvelope, AgentEvent, TurnTrees};
use patchwork_server::{NewThread, Res, Server, orchestration, persistence, provider};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WindowEvent};
use tauri_plugin_notification::NotificationExt;

type Srv<'a> = State<'a, Arc<Server>>;


/// Runs blocking work (CLI probes, network, disk scans) off the main thread.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())
}

// ---- settings & projects -------------------------------------------------

#[tauri::command]
fn get_settings(server: Srv) -> Settings {
    server.store.settings()
}

#[tauri::command]
fn save_settings(app: AppHandle, server: Srv, settings: Settings) -> Res<()> {
    let logo = settings.logo.clone();
    let logo_changed = server.store.settings().logo != logo;
    server.save_settings(settings)?;
    if logo_changed {
        logo::apply(&app, logo.as_deref());
    }
    Ok(())
}

#[tauri::command]
fn list_shells() -> ShellChoices {
    ShellChoices {
        shells: patchwork_server::available_shells(),
        default: patchwork_server::effective_shell(None),
    }
}

#[derive(serde::Serialize)]
struct SkillsView {
    dir: String,
    skills: Vec<patchwork_server::skills::Skill>,
}

/// The skills folder in use and the skills in it.
#[tauri::command]
fn list_skills(server: Srv) -> SkillsView {
    let dir = patchwork_server::skills::dir(&server.store.settings());
    SkillsView {
        skills: patchwork_server::skills::all(&server.store.settings()),
        dir: dir.to_string_lossy().into_owned(),
    }
}

/// Opens the skills folder in the file manager, creating it first.
#[tauri::command]
fn open_skills_folder(app: AppHandle, server: Srv) -> Res<()> {
    open_folder(&app, &patchwork_server::skills::dir(&server.store.settings()))
}

/// Opens the folder of the debug logs in the file manager, creating it first.
#[tauri::command]
fn open_logs_folder(app: AppHandle) -> Res<()> {
    let dir = app.path().config_dir().map_err(|e| e.to_string())?.join("patchwork/logs");
    open_folder(&app, &dir)
}

/// This launch's logs, for the Output panel.
#[tauri::command]
fn log_channels() -> Vec<String> {
    patchwork_server::debug_log::channels()
}

/// A log from `offset` on, for the Output panel to follow.
#[tauri::command]
async fn read_log(channel: String, offset: u64) -> Res<patchwork_server::debug_log::LogChunk> {
    blocking(move || patchwork_server::debug_log::read(&channel, offset)).await?
}

/// A message from the window for `window/renderer.log`.
#[tauri::command]
fn log_renderer(level: patchwork_server::types::LogLevel, message: String) {
    use patchwork_server::debug_log::{Data, RENDERER, Source};
    RENDERER.write(level, "", &Source::default(), Data::Text(message));
}

/// Saves the kept sessions' logs as a zip at `path`, with `system.json`: the app's version,
/// the platform, the connected agents' CLI versions, and the settings, secrets removed.
#[tauri::command]
async fn export_logs(app: AppHandle, server: Srv<'_>, path: String) -> Res<()> {
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

#[cfg_attr(target_os = "linux", allow(unused_variables))]
fn open_folder(app: &AppHandle, dir: &std::path::Path) -> Res<()> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    #[cfg(target_os = "linux")]
    {
        let mut child = patchwork_server::host_command("xdg-open")
            .arg(dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("Could not open the folder: {e}"))?;
        std::thread::spawn(move || child.wait());
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        use tauri_plugin_opener::OpenerExt;
        app.opener()
            .open_path(dir.to_string_lossy(), None::<&str>)
            .map_err(|e| e.to_string())
    }
}

#[tauri::command]
fn list_models(kind: AgentKind) -> Vec<ModelOption> {
    provider::list_models(kind)
}

// ---- API providers -------------------------------------------------------

#[tauri::command]
fn list_api_providers() -> Vec<provider::api_config::ProviderView> {
    provider::api_config::list()
}

#[tauri::command]
async fn save_api_provider(
    provider: provider::api_config::ApiProvider,
    key: provider::api_config::KeyChange,
) -> Res<provider::api_config::ProviderView> {
    blocking(move || provider::api_config::save(provider, key)).await?
}

#[tauri::command]
async fn remove_api_provider(id: String) -> Res<()> {
    blocking(move || provider::api_config::remove(&id)).await?
}

// ---- MCP servers ---------------------------------------------------------

#[tauri::command]
fn list_mcp_servers() -> Vec<provider::mcp::ServerView> {
    provider::mcp::list()
}

#[tauri::command]
async fn save_mcp_server(name: String, previous: Option<String>, server: provider::mcp::McpServer) -> Res<()> {
    blocking(move || provider::mcp::save(name, previous, server)).await?
}

#[tauri::command]
async fn remove_mcp_server(name: String) -> Res<()> {
    blocking(move || provider::mcp::remove(&name)).await?
}

#[tauri::command]
async fn restart_mcp_server(name: String) -> Res<()> {
    blocking(move || provider::mcp::restart(&name)).await
}

/// How many MCP tools an API turn may offer.
#[tauri::command]
fn mcp_tool_budget() -> usize {
    provider::mcp::tool_budget()
}

#[tauri::command]
fn mcp_config_file() -> String {
    provider::mcp::path().display().to_string()
}

/// The MCP servers Claude Code and Codex use, from their own config.
#[tauri::command]
async fn list_cli_mcp_servers(server: Srv<'_>) -> Res<Vec<provider::mcp_clis::CliGroup>> {
    let projects = server.store.settings().projects;
    blocking(move || provider::mcp_clis::list(&projects)).await
}

#[tauri::command]
async fn refresh_cli_mcp_server(server: Srv<'_>, key: String) -> Res<()> {
    let projects = server.store.settings().projects;
    blocking(move || provider::mcp_clis::refresh(&key, &projects)).await
}

#[tauri::command]
async fn enable_cli_mcp_server(server: Srv<'_>, key: String) -> Res<()> {
    let projects = server.store.settings().projects;
    blocking(move || provider::mcp_clis::enable(&key, &projects)).await?
}

/// Opens a file MCP servers are configured in, in the user's editor.
#[tauri::command]
fn open_mcp_config(server: Srv, path: String) -> Res<()> {
    let path = std::path::PathBuf::from(path);
    if !provider::mcp_clis::is_config_file(&path, &server.store.settings().projects) {
        return Err("That is not an MCP config file".into());
    }
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Err("That is not an MCP config file".into());
    };
    workspace::open_in_editor(&dir.to_string_lossy(), &name.to_string_lossy())
}

#[tauri::command]
async fn fetch_api_models(query: provider::api_config::ModelsQuery) -> Res<Vec<provider::api_config::ApiModel>> {
    blocking(move || provider::api_config::fetch_models(query)).await?
}

#[tauri::command]
async fn detect_agent(kind: AgentKind) -> AgentStatus {
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
async fn get_usage(server: Srv<'_>, kind: AgentKind, refresh: bool) -> Res<ProviderUsage> {
    let server = server.inner().clone();
    blocking(move || server.usage.get(kind, refresh)).await
}

#[tauri::command]
fn add_project(server: Srv, path: String) -> Res<Project> {
    server.add_project(&path)
}

#[tauri::command]
fn create_project(server: Srv, parent: String, name: String) -> Res<Project> {
    server.create_project(&parent, &name)
}

#[tauri::command]
fn rename_project(server: Srv, id: String, name: String) -> Res<()> {
    server.rename_project(&id, &name)
}

#[tauri::command]
fn relocate_project(server: Srv, id: String, path: String) -> Res<()> {
    server.relocate_project(&id, &path)
}

#[tauri::command]
fn is_folder(path: String) -> bool {
    std::path::Path::new(&path).is_dir()
}

#[tauri::command]
fn rename_thread(server: Srv, id: String, title: String) -> Res<()> {
    server.rename_thread(&id, &title)
}

#[tauri::command]
fn remove_project(server: Srv, id: String) -> Res<()> {
    server.remove_project(&id)
}

// ---- threads & turns -----------------------------------------------------

#[tauri::command]
fn list_threads(server: Srv) -> Vec<Thread> {
    server.list_threads()
}

#[tauri::command]
fn get_thread(server: Srv, id: String) -> Res<Thread> {
    server.thread(&id)
}

#[tauri::command]
fn reorder_threads(server: Srv, ids: Vec<String>, folder_id: Option<String>) -> Res<()> {
    server.reorder_threads(&ids, folder_id.as_deref())
}

#[tauri::command]
fn list_folders(server: Srv) -> Vec<Folder> {
    server.list_folders()
}

#[tauri::command]
fn create_folder(server: Srv, project_id: String, name: String) -> Res<Folder> {
    server.create_folder(&project_id, &name)
}

#[tauri::command]
fn rename_folder(server: Srv, id: String, name: String) -> Res<()> {
    server.rename_folder(&id, &name)
}

#[tauri::command]
fn remove_folder(server: Srv, id: String) -> Res<()> {
    server.remove_folder(&id)
}

#[tauri::command]
fn list_files(server: Srv, project_id: String) -> Res<Vec<String>> {
    server.list_files(&project_id)
}

#[tauri::command]
fn antigravity_installed(server: Srv) -> bool {
    server.antigravity_installed()
}

#[tauri::command]
async fn install_antigravity(server: Srv<'_>) -> Res<()> {
    let server = server.inner().clone();
    tauri::async_runtime::spawn_blocking(move || server.install_antigravity())
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn clear_thread(server: Srv, id: String) -> Res<()> {
    server.clear_thread(&id)
}

#[tauri::command]
fn checkpoints(server: Srv, thread_id: String) -> Vec<String> {
    server.checkpoints(&thread_id)
}

#[tauri::command]
fn restore_checkpoint(server: Srv, thread_id: String, message_id: String) -> Res<Thread> {
    server.restore_checkpoint(&thread_id, &message_id)
}

#[tauri::command]
fn get_last_thread(server: Srv) -> Option<String> {
    server.last_thread_id()
}

#[tauri::command]
fn set_last_thread(server: Srv, id: String) -> Res<()> {
    server.set_last_thread_id(&id)
}

#[tauri::command]
fn get_collapsed(server: Srv) -> Vec<String> {
    server.collapsed_sections()
}

#[tauri::command]
fn set_collapsed(server: Srv, ids: Vec<String>) -> Res<()> {
    server.set_collapsed_sections(&ids)
}

#[tauri::command]
fn create_thread(server: Srv, request: NewThread) -> Res<Thread> {
    server.create_thread(request)
}

#[tauri::command]
fn delete_thread(server: Srv, id: String) {
    server.delete_thread(&id)
}

#[tauri::command]
fn set_thread_archived(server: Srv, id: String, archived: bool) -> Res<()> {
    server.set_thread_archived(&id, archived)
}

#[tauri::command]
fn set_thread_model(server: Srv, id: String, agent: AgentKind, model: Option<String>) -> Res<()> {
    server.set_thread_model(&id, agent, model)
}

#[tauri::command]
fn set_thread_access_mode(server: Srv, id: String, mode: AccessMode) -> Res<()> {
    server.set_thread_access_mode(&id, mode)
}

#[tauri::command]
fn save_text_file(server: Srv, path: String, text: String) -> Res<()> {
    server.save_text_file(&path, &text)
}

#[tauri::command]
fn set_thread_plan_mode(server: Srv, id: String, on: bool) -> Res<()> {
    server.set_thread_plan_mode(&id, on)
}

#[tauri::command]
fn set_thread_effort(server: Srv, id: String, effort: Option<String>) -> Res<()> {
    server.set_thread_effort(&id, effort)
}

/// Off the main thread: a steer can wait a few seconds for Claude to background its subagents.
#[tauri::command]
async fn send_message(
    server: Srv<'_>,
    thread_id: String,
    text: String,
    attachments: Vec<Attachment>,
    queue: bool,
) -> Res<Message> {
    let server = server.inner().clone();
    blocking(move || server.send_message(thread_id, text, attachments, queue)).await?
}

#[tauri::command]
fn attachment_for_path(path: String) -> Res<Attachment> {
    Attachment::from_path(&path)
}

#[tauri::command]
fn save_pasted_image(server: Srv, mime_type: String, data: String) -> Res<Attachment> {
    server.save_pasted_image(&mime_type, &data)
}

#[tauri::command]
fn save_pasted_text(server: Srv, text: String) -> Res<Attachment> {
    server.save_pasted_text(&text)
}

/// The `vscode-editor-data` of the clipboard's copy, which holds its language when it came
/// from VS Code or another Electron editor. Runs off the main thread, since reading the
/// clipboard waits on the app that owns it.
#[tauri::command(async)]
fn clipboard_editor_data() -> Option<String> {
    clipboard::editor_data()
}

/// Lets the webview load an image for its thumbnail. The asset scope is not kept across restarts,
/// so this runs again whenever a thumbnail is shown.
#[tauri::command]
fn allow_preview(app: tauri::AppHandle, path: String) -> Res<()> {
    app.asset_protocol_scope()
        .allow_file(&path)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn cancel_turn(server: Srv, thread_id: String) {
    server.cancel_turn(&thread_id)
}

#[tauri::command]
fn stop_subagent(server: Srv, thread_id: String, tool_id: String) -> Res<()> {
    server.stop_subagent(&thread_id, &tool_id)
}

#[tauri::command]
fn remove_queued(server: Srv, thread_id: String, message_id: String) -> Res<()> {
    server.remove_queued(&thread_id, &message_id)
}

#[tauri::command]
fn edit_queued(server: Srv, thread_id: String, message_id: String, text: String, attachments: Vec<Attachment>) -> Res<()> {
    server.edit_queued(&thread_id, &message_id, text, attachments)
}

#[tauri::command]
async fn resume_queued(server: Srv<'_>, thread_id: String) -> Res<()> {
    let server = server.inner().clone();
    tauri::async_runtime::spawn_blocking(move || server.resume_queued(&thread_id))
        .await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn steer_queued(server: Srv<'_>, thread_id: String, message_id: String) -> Res<()> {
    let server = server.inner().clone();
    tauri::async_runtime::spawn_blocking(move || server.steer_queued(&thread_id, &message_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn stop_goal(server: Srv<'_>, thread_id: String) -> Res<()> {
    // Starts the turn that clears the agent's goal, so it runs off the main thread.
    let server = server.inner().clone();
    tauri::async_runtime::spawn_blocking(move || server.stop_goal(&thread_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn respond_approval(
    server: Srv,
    thread_id: String,
    approval_id: String,
    allow: bool,
    answers: Option<std::collections::HashMap<String, Vec<String>>>,
) -> Res<()> {
    server.respond_approval(&thread_id, &approval_id, allow, answers.unwrap_or_default())
}

#[tauri::command]
async fn scan_imports(server: Srv<'_>, kind: ImportSource) -> Res<ImportScan> {
    let server = server.inner().clone();
    blocking(move || server.scan_imports(kind)).await
}

#[tauri::command]
async fn import_threads(server: Srv<'_>, kinds: Vec<ImportSource>) -> Res<u32> {
    let server = server.inner().clone();
    blocking(move || server.import_threads(&kinds)).await?
}

// ---- files & git ---------------------------------------------------------

use patchwork_server::workspace;

#[tauri::command]
async fn missing_files(server: Srv<'_>, project_id: String, paths: Vec<String>) -> Res<Vec<String>> {
    let root = server.project_root(&project_id)?;
    blocking(move || workspace::missing_files(&root, &paths)).await
}

/// What a reply's turn did to each of `paths`, and where each file stands now.
#[tauri::command]
async fn turn_files(
    server: Srv<'_>,
    project_id: String,
    turn: TurnTrees,
    paths: Vec<String>,
) -> Res<Vec<orchestration::checkpoint::TurnFile>> {
    let root = server.project_root(&project_id)?;
    blocking(move || orchestration::checkpoint::turn_files(&root, &turn, &paths)).await?
}

/// A reply's turn's diff of one file.
#[tauri::command]
async fn turn_file_diff(server: Srv<'_>, project_id: String, turn: TurnTrees, path: String) -> Res<String> {
    let root = server.project_root(&project_id)?;
    blocking(move || orchestration::checkpoint::turn_file_diff(&root, &turn, &path)).await?
}

#[tauri::command]
async fn read_file(server: Srv<'_>, project_id: String, path: String) -> Res<String> {
    let root = server.project_root(&project_id)?;
    blocking(move || workspace::read_file(&root, &path)).await?
}

#[tauri::command]
async fn git_branch(server: Srv<'_>, project_id: String) -> Res<Option<String>> {
    let root = server.project_root(&project_id)?;
    blocking(move || workspace::git_branch(&root)).await
}

#[tauri::command]
async fn git_changes(server: Srv<'_>, project_id: String) -> Res<Vec<FileChange>> {
    let root = server.project_root(&project_id)?;
    blocking(move || workspace::git_changes(&root)).await?
}

#[tauri::command]
async fn git_file_diff(server: Srv<'_>, project_id: String, path: String) -> Res<String> {
    let root = server.project_root(&project_id)?;
    blocking(move || workspace::git_file_diff(&root, &path)).await?
}

/// Watches the active project in place of any other, emitting `project-changed` after each
/// settled burst of changes.
#[tauri::command]
fn watch_project(app: AppHandle, server: Srv, project_id: String) -> Res<()> {
    server.watch_project(&project_id, move |changed| {
        let _ = app.emit("project-changed", changed);
    })
}

#[tauri::command]
fn accept_file(server: Srv, project_id: String, path: String) -> Res<()> {
    workspace::accept_file(&server.project_root(&project_id)?, &path)
}

#[tauri::command]
fn revert_file(server: Srv, project_id: String, path: String) -> Res<()> {
    workspace::revert_file(&server.project_root(&project_id)?, &path)
}

#[tauri::command]
fn open_in_editor(server: Srv, project_id: String, path: String) -> Res<()> {
    workspace::open_in_editor(&server.project_root(&project_id)?, &path)
}

/// Opens a web or mail link in the user's default app. On Linux `xdg-open` runs with the user's
/// own environment, where the opener plugin would pass on the AppImage's.
#[tauri::command]
#[cfg_attr(target_os = "linux", allow(unused_variables))]
fn open_url(app: AppHandle, url: String) -> Res<()> {
    let scheme = url.split_once(':').map(|(scheme, _)| scheme.to_ascii_lowercase());
    if !matches!(scheme.as_deref(), Some("http" | "https" | "mailto" | "tel")) {
        return Err("Patchwork only opens web and mail links".into());
    }
    #[cfg(target_os = "linux")]
    {
        let mut child = patchwork_server::host_command("xdg-open")
            .arg(&url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("Could not open the link: {e}"))?;
        std::thread::spawn(move || child.wait());
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        use tauri_plugin_opener::OpenerExt;
        app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
    }
}

// ---- updates -------------------------------------------------------------

#[tauri::command]
fn update_status(app: AppHandle, updates: State<updates::Updates>) -> updates::UpdateStatus {
    updates.status(&app)
}

/// Starts a check. Its progress and result arrive on `"update-status"`.
#[tauri::command]
fn check_for_updates(app: AppHandle) {
    std::thread::spawn(move || app.state::<updates::Updates>().check(&app, true));
}

#[tauri::command]
fn restart_to_update(app: AppHandle, updates: State<updates::Updates>, force: bool) -> updates::RestartResult {
    updates.restart(&app, force)
}

// ---- tray & notifications ----------------------------------------------

/// Whether the tray icon is up, so closing the window hides it instead of quitting.
struct TrayAvailable(bool);

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        // Wayland ignores unminimize and focus requests, so remap the window to bring it forward.
        let _ = window.hide();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn create_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show Patchwork", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let mut tray = TrayIconBuilder::with_id(logo::TRAY_ID)
        .menu(&Menu::with_items(app, &[&show, &quit])?)
        .tooltip("Patchwork")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// Raises desktop notifications for finished goals and tasks while the window is not focused.
/// Only the user's own tasks notify: not a subagent's thread, an archived task, or upkeep the app
/// runs by itself.
#[derive(Default)]
struct Notifier {
    /// Each task's goal status as last reported this session. A goal notifies when it is met only
    /// if it was seen being pursued, so an agent restating a goal that already ended stays quiet.
    goals: Mutex<HashMap<String, GoalStatus>>,
    /// Tasks whose goal was just met, so the turn that met it does not notify a second time.
    goal_met: Mutex<HashSet<String>>,
}

impl Notifier {
    fn on_event(&self, app: &AppHandle, envelope: &AgentEventEnvelope) {
        let Some(server) = app.try_state::<Arc<Server>>() else {
            return;
        };
        // Streamed text and reasoning never notify, and come per token: they skip the settings
        // copy, made under the store's lock.
        if matches!(
            envelope.event,
            AgentEvent::TextDelta { .. } | AgentEvent::Thinking { .. }
        ) {
            return;
        }
        let settings = server.store.settings().notifications;
        let thread_id = &envelope.thread_id;
        let mut goal_met = self.goal_met.lock().unwrap_or_else(PoisonError::into_inner);
        let (title, body) = match &envelope.event {
            AgentEvent::GoalUpdated { goal } => {
                let mut goals = self.goals.lock().unwrap_or_else(PoisonError::into_inner);
                let before = match goal {
                    Some(goal) => goals.insert(thread_id.clone(), goal.status),
                    None => goals.remove(thread_id),
                };
                let pursued = matches!(before, Some(GoalStatus::Active | GoalStatus::Paused));
                let Some(goal) = goal.as_ref().filter(|g| g.status == GoalStatus::Complete) else {
                    return;
                };
                if !pursued {
                    return;
                }
                goal_met.insert(thread_id.clone());
                if !settings.goal_complete {
                    return;
                }
                ("Goal complete", goal.objective.clone())
            }
            AgentEvent::TurnCompleted {
                status: MessageStatus::Complete,
                ..
            } => {
                if goal_met.remove(thread_id) || !settings.task_complete {
                    return;
                }
                let Some(thread) = server.store.thread(thread_id) else {
                    return;
                };
                // A steered turn completes each of its replies in turn; the last one notifies.
                let still_running = thread
                    .messages
                    .iter()
                    .any(|m| m.status == MessageStatus::Streaming);
                let asked = thread.messages.iter().rev().find(|m| m.role == Role::User);
                if thread.parent.is_some()
                    || thread.archived_at.is_some()
                    || still_running
                    || asked.is_some_and(|m| orchestration::is_upkeep(&m.text))
                    // A goal's turns keep coming until it is met; only its outcome notifies.
                    || thread.goal.is_some_and(|g| g.status == GoalStatus::Active)
                {
                    return;
                }
                ("Task complete", thread.title)
            }
            _ => return,
        };
        drop(goal_met);
        let focused = app.get_webview_window("main").is_some_and(|window| {
            window.is_visible().unwrap_or(false) && window.is_focused().unwrap_or(false)
        });
        if !focused {
            let mut notification = app.notification().builder().title(title).body(body);
            // Without an icon, Linux looks up one named after the executable, which is not installed.
            if let Some(icon) = logo::notification_icon(app, server.store.settings().logo.as_deref()) {
                notification = notification.icon(icon);
            }
            let _ = notification.show();
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event
                && window.state::<TrayAvailable>().0
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(|app| {
            // Config lives in `~/.config/patchwork` rather than the app-identifier folder.
            let config_dir = app.path().config_dir()?.join("patchwork");
            // The app's own messages go to `main.log` in this launch's log folder in every
            // build, as VS Code writes its own, and to stdout in development. Settings →
            // Logging sets how much they record.
            {
                use tauri_plugin_log::{Builder, RotationStrategy, Target, TargetKind};
                let mut targets = vec![Target::new(TargetKind::Folder {
                    path: patchwork_server::debug_log::session_dir(&config_dir),
                    file_name: Some("main".into()),
                })];
                if cfg!(debug_assertions) {
                    targets.push(Target::new(TargetKind::Stdout));
                }
                app.handle().plugin(
                    Builder::new()
                        .targets(targets)
                        // Each message is checked against the setting, which the server sets
                        // once it opens.
                        .level(log::LevelFilter::Trace)
                        .filter(|metadata| patchwork_server::debug_log::app_logs(metadata.level()))
                        .format(|out, message, record| {
                            let level = match record.level() {
                                log::Level::Warn => "warning".to_string(),
                                level => level.as_str().to_lowercase(),
                            };
                            let time = patchwork_server::debug_log::now_text();
                            out.finish(format_args!("{time} [{level}] {message}"))
                        })
                        .max_file_size(5 * 1024 * 1024)
                        .rotation_strategy(RotationStrategy::KeepSome(5))
                        .build(),
                )?;
                // A panic is logged before the default hook reports it.
                let report = std::panic::take_hook();
                std::panic::set_hook(Box::new(move |info| {
                    log::error!("panic: {info}");
                    report(info);
                }));
            }
            persistence::adopt_old_dir(&app.path().app_config_dir()?, &config_dir)?;
            let handle = app.handle().clone();
            let notifier = Notifier::default();
            let server = Server::open(
                config_dir,
                &app.path().app_local_data_dir()?,
                Box::new(move |event| {
                    notifier.on_event(&handle, &event);
                    let _ = handle.emit("agent-event", event);
                }),
            )?;
            server.start_idle_compaction();
            server.continue_interrupted();
            let logo = server.store.settings().logo;
            app.manage(server);
            let tray = create_tray(app);
            if let Err(e) = &tray {
                log::warn!("system tray unavailable, closing the window quits: {e}");
            }
            app.manage(TrayAvailable(tray.is_ok()));
            app.manage(logo::LogoChanges::default());
            logo::apply(app.handle(), logo.as_deref());
            app.manage(updates::Updates::default());
            app.manage(terminal::Terminals::default());
            updates::start(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            detect_agent,
            list_shells,
            list_skills,
            open_skills_folder,
            open_logs_folder,
            log_channels,
            read_log,
            log_renderer,
            terminal::terminal_spawn,
            terminal::terminal_write,
            terminal::terminal_resize,
            terminal::terminal_kill,
            export_logs,
            list_models,
            list_api_providers,
            save_api_provider,
            remove_api_provider,
            list_mcp_servers,
            save_mcp_server,
            remove_mcp_server,
            restart_mcp_server,
            mcp_config_file,
            mcp_tool_budget,
            list_cli_mcp_servers,
            refresh_cli_mcp_server,
            enable_cli_mcp_server,
            open_mcp_config,
            fetch_api_models,
            get_usage,
            add_project,
            create_project,
            rename_project,
            relocate_project,
            is_folder,
            rename_thread,
            remove_project,
            list_threads,
            get_thread,
            reorder_threads,
            list_folders,
            create_folder,
            rename_folder,
            remove_folder,
            list_files,
            antigravity_installed,
            install_antigravity,
            clear_thread,
            checkpoints,
            restore_checkpoint,
            get_last_thread,
            set_last_thread,
            get_collapsed,
            set_collapsed,
            create_thread,
            delete_thread,
            set_thread_archived,
            set_thread_model,
            set_thread_access_mode,
            set_thread_plan_mode,
            save_text_file,
            set_thread_effort,
            send_message,
            attachment_for_path,
            save_pasted_image,
            save_pasted_text,
            clipboard_editor_data,
            allow_preview,
            cancel_turn,
            stop_goal,
            stop_subagent,
            remove_queued,
            edit_queued,
            resume_queued,
            steer_queued,
            respond_approval,
            scan_imports,
            import_threads,
            read_file,
            missing_files,
            turn_files,
            turn_file_diff,
            git_branch,
            git_changes,
            git_file_diff,
            watch_project,
            accept_file,
            revert_file,
            open_in_editor,
            open_url,
            update_status,
            check_for_updates,
            restart_to_update,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Patchwork")
        .run(|app, event| {
            // Agents run in their own scopes, so they would outlive the app.
            if let RunEvent::Exit = event {
                if let Some(server) = app.try_state::<Arc<Server>>() {
                    server.stop_agents();
                }
                // After the agents, since a Windows update runs its installer and exits.
                if let Some(updates) = app.try_state::<updates::Updates>() {
                    updates.install_on_exit();
                }
            }
        });
}
