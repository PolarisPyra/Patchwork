//! Tauri shell: exposes `patchwork_server::Server` as commands and forwards
//! agent events to the web UI on the `"agent-event"` channel.

mod clipboard;
mod logo;
mod terminal;
mod updates;
mod commands;

use std::sync::{Arc, Mutex, PoisonError};

use patchwork_server::types::{AgentEvent, AgentEventEnvelope, GoalStatus, MessageStatus, Role};
use std::collections::{HashMap, HashSet};

use patchwork_server::{Server, orchestration, persistence};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, RunEvent, WindowEvent};
use tauri_plugin_notification::NotificationExt;

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
            if let Some(icon) =
                logo::notification_icon(app, server.store.settings().logo.as_deref())
            {
                notification = notification.icon(icon);
            }
            let _ = notification.show();
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// How often a scheduled task is looked for. A tick that finds nothing to start does no work.
const SCHEDULED_TICK: std::time::Duration = std::time::Duration::from_secs(30);

/// Starts the tasks whose time has come, on a timer of its own, for the life of the app.
fn start_scheduled_tasks(server: Arc<Server>) {
    std::thread::Builder::new()
        .name("scheduled-tasks".into())
        .spawn(move || {
            loop {
                for name in server.run_due_scheduled_tasks() {
                    log::info!("scheduled task started: {name}");
                }
                std::thread::sleep(SCHEDULED_TICK);
            }
        })
        .map(|_| ())
        .unwrap_or_else(|e| log::warn!("scheduled tasks will not run: {e}"));
}

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
            start_scheduled_tasks(Arc::clone(&server));
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
            commands::settings::get_settings,
            commands::scheduled::list_scheduled_tasks,
            commands::scheduled::save_scheduled_task,
            commands::scheduled::remove_scheduled_task,
            commands::scheduled::run_scheduled_task,
            commands::plugins::list_plugins,
            commands::plugins::open_plugins_folder,
            commands::codex::codex_info,
            commands::codex::codex_command,
            commands::computer::computer_screenshot,
            commands::computer::computer_action,
            commands::settings::default_permission_rules,
            commands::settings::save_settings,
            commands::mcp::detect_agent,
            commands::settings::list_shells,
            commands::settings::list_skills,
            commands::settings::open_skills_folder,
            commands::settings::watch_skills,
            commands::settings::unwatch_skills,
            commands::settings::open_skill,
            commands::settings::watch_agents,
            commands::settings::open_agent,
            commands::settings::list_agents,
            commands::settings::create_agent,
            commands::settings::open_logs_folder,
            commands::settings::log_channels,
            commands::settings::read_log,
            commands::settings::log_renderer,
            terminal::terminal_spawn,
            terminal::terminal_write,
            terminal::terminal_resize,
            terminal::terminal_kill,
            commands::settings::export_logs,
            commands::list_models,
            commands::providers::list_api_providers,
            commands::providers::save_api_provider,
            commands::providers::remove_api_provider,
            commands::mcp::list_mcp_servers,
            commands::mcp::save_mcp_server,
            commands::mcp::remove_mcp_server,
            commands::mcp::restart_mcp_server,
            commands::mcp::mcp_config_file,
            commands::mcp::mcp_tool_budget,
            commands::mcp::list_cli_mcp_servers,
            commands::mcp::refresh_cli_mcp_server,
            commands::mcp::enable_cli_mcp_server,
            commands::mcp::open_mcp_config,
            commands::mcp::fetch_api_models,
            commands::mcp::get_usage,
            commands::mcp::add_project,
            commands::mcp::create_project,
            commands::mcp::rename_project,
            commands::mcp::relocate_project,
            commands::mcp::is_folder,
            commands::mcp::rename_thread,
            commands::mcp::remove_project,
            commands::threads::list_threads,
            commands::threads::get_thread,
            commands::threads::reorder_threads,
            commands::threads::list_folders,
            commands::threads::create_folder,
            commands::threads::rename_folder,
            commands::threads::remove_folder,
            commands::threads::list_files,
            commands::threads::antigravity_installed,
            commands::threads::install_antigravity,
            commands::threads::clear_thread,
            commands::threads::checkpoints,
            commands::threads::restore_checkpoint,
            commands::threads::get_last_thread,
            commands::threads::set_last_thread,
            commands::threads::get_collapsed,
            commands::threads::set_collapsed,
            commands::threads::create_thread,
            commands::threads::delete_thread,
            commands::threads::set_thread_archived,
            commands::threads::set_thread_model,
            commands::threads::set_thread_access_mode,
            commands::threads::set_thread_plan_mode,
            commands::threads::set_thread_role,
            commands::threads::save_text_file,
            commands::threads::set_thread_effort,
            commands::threads::send_message,
            commands::threads::attachment_for_path,
            commands::threads::save_pasted_image,
            commands::threads::save_pasted_text,
            commands::threads::clipboard_editor_data,
            commands::threads::paste_clipboard_image,
            commands::threads::clipboard_files,
            commands::threads::copy_text,
            commands::threads::allow_preview,
            commands::threads::cancel_turn,
            commands::threads::stop_goal,
            commands::threads::stop_subagent,
            commands::threads::remove_queued,
            commands::threads::edit_queued,
            commands::threads::resume_queued,
            commands::threads::steer_queued,
            commands::threads::respond_approval,
            commands::threads::scan_imports,
            commands::threads::import_threads,
            commands::files::read_file,
            commands::files::missing_files,
            commands::files::file_media,
            commands::files::open_file,
            commands::files::turn_files,
            commands::files::turn_file_diff,
            commands::files::git_branch,
            commands::files::git_changes,
            commands::files::git_file_diff,
            commands::files::watch_project,
            commands::files::accept_file,
            commands::files::revert_file,
            commands::files::open_in_editor,
            commands::files::reveal_file,
            commands::files::open_url,
            commands::app::update_status,
            commands::app::check_for_updates,
            commands::app::restart_to_update,
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
