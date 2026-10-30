//! Threads: creating, listing, changing, and removing them, and their checkpoints.

use super::*;

#[tauri::command]
pub(crate) fn list_threads(server: Srv) -> Vec<Thread> {
    server.list_threads()
}

#[tauri::command]
pub(crate) fn get_thread(server: Srv, id: String) -> Res<Thread> {
    server.thread(&id)
}

#[tauri::command]
pub(crate) fn reorder_threads(server: Srv, ids: Vec<String>, folder_id: Option<String>) -> Res<()> {
    server.reorder_threads(&ids, folder_id.as_deref())
}

#[tauri::command]
pub(crate) fn create_thread(server: Srv, request: NewThread) -> Res<Thread> {
    server.create_thread(request)
}

#[tauri::command]
pub(crate) fn delete_thread(server: Srv, id: String) {
    server.delete_thread(&id)
}

#[tauri::command]
pub(crate) fn clear_thread(server: Srv, id: String) -> Res<()> {
    server.clear_thread(&id)
}

#[tauri::command]
pub(crate) fn set_thread_archived(server: Srv, id: String, archived: bool) -> Res<()> {
    server.set_thread_archived(&id, archived)
}

#[tauri::command]
pub(crate) fn rename_thread(server: Srv, id: String, title: String) -> Res<()> {
    server.rename_thread(&id, &title)
}

#[tauri::command]
pub(crate) fn set_thread_model(
    server: Srv,
    id: String,
    agent: AgentKind,
    model: Option<String>,
) -> Res<()> {
    server.set_thread_model(&id, agent, model)
}

#[tauri::command]
pub(crate) fn set_thread_access_mode(server: Srv, id: String, mode: AccessMode) -> Res<()> {
    server.set_thread_access_mode(&id, mode)
}

#[tauri::command]
pub(crate) fn set_thread_plan_mode(server: Srv, id: String, on: bool) -> Res<()> {
    server.set_thread_plan_mode(&id, on)
}

#[tauri::command]
pub(crate) fn set_thread_effort(server: Srv, id: String, effort: Option<String>) -> Res<()> {
    server.set_thread_effort(&id, effort)
}

#[tauri::command]
pub(crate) fn set_thread_compact_window(server: Srv, id: String, window: Option<u64>) -> Res<()> {
    server.set_thread_compact_window(&id, window)
}

#[tauri::command]
pub(crate) fn set_thread_role(server: Srv, id: String, role: Option<String>) -> Res<()> {
    server.set_thread_role(&id, role)
}

#[tauri::command]
pub(crate) fn checkpoints(server: Srv, thread_id: String) -> Vec<String> {
    server.checkpoints(&thread_id)
}

#[tauri::command]
pub(crate) fn restore_checkpoint(
    server: Srv,
    thread_id: String,
    message_id: String,
) -> Res<Thread> {
    server.restore_checkpoint(&thread_id, &message_id)
}

#[tauri::command]
pub(crate) async fn fork_thread(
    server: Srv<'_>,
    thread_id: String,
    message_id: String,
) -> Res<Thread> {
    let server = server.inner().clone();
    blocking(move || server.fork_thread(&thread_id, &message_id)).await?
}

#[tauri::command]
pub(crate) fn get_last_thread(server: Srv) -> Option<String> {
    server.last_thread_id()
}

#[tauri::command]
pub(crate) fn set_last_thread(server: Srv, id: String) -> Res<()> {
    server.set_last_thread_id(&id)
}
