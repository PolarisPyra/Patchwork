//! Turns: sending messages, the queue, steering, approvals, goals, and stopping.

use super::*;

/// Off the main thread: a steer can wait a few seconds for Claude to background its subagents.
#[tauri::command]
pub(crate) async fn send_message(
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
pub(crate) fn cancel_turn(server: Srv, thread_id: String) {
    server.cancel_turn(&thread_id)
}

#[tauri::command]
pub(crate) fn stop_subagent(server: Srv, thread_id: String, tool_id: String) -> Res<()> {
    server.stop_subagent(&thread_id, &tool_id)
}

#[tauri::command]
pub(crate) fn remove_queued(server: Srv, thread_id: String, message_id: String) -> Res<()> {
    server.remove_queued(&thread_id, &message_id)
}

#[tauri::command]
pub(crate) fn edit_queued(
    server: Srv,
    thread_id: String,
    message_id: String,
    text: String,
    attachments: Vec<Attachment>,
) -> Res<()> {
    server.edit_queued(&thread_id, &message_id, text, attachments)
}

#[tauri::command]
pub(crate) async fn resume_queued(server: Srv<'_>, thread_id: String) -> Res<()> {
    let server = server.inner().clone();
    tauri::async_runtime::spawn_blocking(move || server.resume_queued(&thread_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn steer_queued(
    server: Srv<'_>,
    thread_id: String,
    message_id: String,
) -> Res<()> {
    let server = server.inner().clone();
    tauri::async_runtime::spawn_blocking(move || server.steer_queued(&thread_id, &message_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn stop_goal(server: Srv<'_>, thread_id: String) -> Res<()> {
    // Starts the turn that clears the agent's goal, so it runs off the main thread.
    let server = server.inner().clone();
    tauri::async_runtime::spawn_blocking(move || server.stop_goal(&thread_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) fn respond_approval(
    server: Srv,
    thread_id: String,
    approval_id: String,
    allow: bool,
    answers: Option<std::collections::HashMap<String, Vec<String>>>,
    rule: Option<patchwork_server::permissions::RuleRef>,
) -> Res<()> {
    server.respond_approval(
        &thread_id,
        &approval_id,
        allow,
        answers.unwrap_or_default(),
        rule,
    )
}

/// Runs the project's check command again for a reply, on a thread of its own: the result
/// arrives as a `checksRecorded` event.
#[tauri::command]
pub(crate) fn run_checks(server: Srv<'_>, thread_id: String, message_id: String) -> Res<()> {
    server.inner().clone().run_checks(&thread_id, &message_id)
}
