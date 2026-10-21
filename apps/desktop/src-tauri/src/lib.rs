//! Tauri shell: exposes `patchwork_server::Server` as commands and forwards
//! agent events to the web UI on the `"agent-event"` channel.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use patchwork_server::types::*;
use patchwork_server::{Res, Server, persistence, provider};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WindowEvent};
use tauri_plugin_notification::NotificationExt;

type Srv<'a> = State<'a, Arc<Server>>;

/// Holds the watcher for the project the canvas has open. Replacing it stops the old one.
#[derive(Default)]
struct ProjectWatch(Mutex<Option<RecommendedWatcher>>);

/// Build output and git internals change constantly and never affect what the canvas shows.
const IGNORED_DIRS: [&str; 3] = [".git", "target", "node_modules"];

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
fn save_settings(server: Srv, settings: Settings) -> Res<()> {
    server.save_settings(settings)
}

#[tauri::command]
fn list_shells() -> ShellChoices {
    ShellChoices {
        shells: patchwork_server::available_shells(),
        default: patchwork_server::effective_shell(None),
    }
}

#[tauri::command]
fn list_models(kind: AgentKind) -> Vec<ModelOption> {
    provider::list_models(kind)
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
fn create_thread(
    server: Srv,
    project_id: Option<String>,
    agent: AgentKind,
    model: Option<String>,
    access_mode: AccessMode,
    effort: Option<String>,
) -> Res<Thread> {
    server.create_thread(project_id, agent, model, access_mode, effort)
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
async fn stop_goal(server: Srv<'_>, thread_id: String) -> Res<()> {
    // Waits for the stopped turn to end, so it runs off the main thread.
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
fn read_file(server: Srv, project_id: String, path: String) -> Res<String> {
    workspace::read_file(&server.project_root(&project_id)?, &path)
}

#[tauri::command]
fn git_branch(server: Srv, project_id: String) -> Res<Option<String>> {
    Ok(workspace::git_branch(&server.project_root(&project_id)?))
}

#[tauri::command]
fn git_changes(server: Srv, project_id: String) -> Res<Vec<FileChange>> {
    workspace::git_changes(&server.project_root(&project_id)?)
}

#[tauri::command]
fn git_file_diff(server: Srv, project_id: String, path: String) -> Res<String> {
    workspace::git_file_diff(&server.project_root(&project_id)?, &path)
}

/// Emits `project-files-changed` with the project id when a file under the project changes.
#[tauri::command]
fn watch_project(
    app: AppHandle,
    server: Srv,
    watch: State<ProjectWatch>,
    project_id: String,
) -> Res<()> {
    let root = PathBuf::from(server.project_root(&project_id)?);
    let event_root = root.clone();
    let watched_id = project_id.clone();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(event) = res else { return };
        if matches!(event.kind, EventKind::Access(_)) {
            return;
        }
        let relevant = event.paths.iter().any(|path| {
            path.strip_prefix(&event_root).is_ok_and(|rel| {
                !rel.components()
                    .any(|c| IGNORED_DIRS.iter().any(|dir| c.as_os_str() == *dir))
            })
        });
        if relevant {
            let _ = app.emit("project-files-changed", &watched_id);
        }
    })
    .map_err(|e| e.to_string())?;
    watcher
        .watch(&root, RecursiveMode::Recursive)
        .map_err(|e| e.to_string())?;
    *watch.0.lock().map_err(|e| e.to_string())? = Some(watcher);
    Ok(())
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
    let mut tray = TrayIconBuilder::new()
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
#[derive(Default)]
struct Notifier {
    /// Tasks whose goal was just met, so the turn that met it does not notify a second time.
    goal_met: Mutex<HashSet<String>>,
}

impl Notifier {
    fn on_event(&self, app: &AppHandle, envelope: &AgentEventEnvelope) {
        let Some(server) = app.try_state::<Arc<Server>>() else {
            return;
        };
        let settings = server.store.settings().notifications;
        let thread_id = &envelope.thread_id;
        let mut goal_met = self.goal_met.lock().unwrap_or_else(PoisonError::into_inner);
        let (title, body) = match &envelope.event {
            AgentEvent::GoalUpdated { goal: Some(goal) } if goal.status == GoalStatus::Complete => {
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
                // A goal's turns keep coming until it is met; only its outcome notifies.
                if thread.goal.is_some_and(|g| g.status == GoalStatus::Active) {
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
            let _ = app.notification().builder().title(title).body(body).show();
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event
                && window.state::<TrayAvailable>().0
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            // Config lives in `~/.config/patchwork` rather than the app-identifier folder.
            let config_dir = app.path().config_dir()?.join("patchwork");
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
            app.manage(server);
            app.manage(ProjectWatch::default());
            let tray = create_tray(app);
            if let Err(e) = &tray {
                log::warn!("system tray unavailable, closing the window quits: {e}");
            }
            app.manage(TrayAvailable(tray.is_ok()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            detect_agent,
            list_shells,
            list_models,
            get_usage,
            add_project,
            create_project,
            rename_project,
            rename_thread,
            remove_project,
            list_threads,
            reorder_threads,
            list_folders,
            create_folder,
            rename_folder,
            remove_folder,
            list_files,
            antigravity_installed,
            install_antigravity,
            clear_thread,
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
            allow_preview,
            cancel_turn,
            stop_goal,
            stop_subagent,
            remove_queued,
            respond_approval,
            scan_imports,
            import_threads,
            read_file,
            git_branch,
            git_changes,
            git_file_diff,
            watch_project,
            accept_file,
            revert_file,
            open_in_editor,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Patchwork")
        .run(|app, event| {
            // Agents run in their own scopes, so they would outlive the app.
            if let RunEvent::Exit = event
                && let Some(server) = app.try_state::<Arc<Server>>()
            {
                server.stop_agents();
            }
        });
}
