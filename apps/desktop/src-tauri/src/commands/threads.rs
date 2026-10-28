//! Threads, turns, messages, and the goal and queue of a thread.

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
pub(crate) fn list_folders(server: Srv) -> Vec<Folder> {
    server.list_folders()
}

#[tauri::command]
pub(crate) fn create_folder(server: Srv, project_id: String, name: String) -> Res<Folder> {
    server.create_folder(&project_id, &name)
}

#[tauri::command]
pub(crate) fn rename_folder(server: Srv, id: String, name: String) -> Res<()> {
    server.rename_folder(&id, &name)
}

#[tauri::command]
pub(crate) fn remove_folder(server: Srv, id: String) -> Res<()> {
    server.remove_folder(&id)
}

#[tauri::command]
pub(crate) fn list_files(server: Srv, project_id: String) -> Res<Vec<String>> {
    server.list_files(&project_id)
}

#[tauri::command]
pub(crate) fn antigravity_installed(server: Srv) -> bool {
    server.antigravity_installed()
}

#[tauri::command]
pub(crate) async fn install_antigravity(server: Srv<'_>) -> Res<()> {
    let server = server.inner().clone();
    tauri::async_runtime::spawn_blocking(move || server.install_antigravity())
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) fn clear_thread(server: Srv, id: String) -> Res<()> {
    server.clear_thread(&id)
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
pub(crate) fn get_last_thread(server: Srv) -> Option<String> {
    server.last_thread_id()
}

#[tauri::command]
pub(crate) fn set_last_thread(server: Srv, id: String) -> Res<()> {
    server.set_last_thread_id(&id)
}

#[tauri::command]
pub(crate) fn get_collapsed(server: Srv) -> Vec<String> {
    server.collapsed_sections()
}

#[tauri::command]
pub(crate) fn set_collapsed(server: Srv, ids: Vec<String>) -> Res<()> {
    server.set_collapsed_sections(&ids)
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
pub(crate) fn set_thread_archived(server: Srv, id: String, archived: bool) -> Res<()> {
    server.set_thread_archived(&id, archived)
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
pub(crate) fn save_text_file(server: Srv, path: String, text: String) -> Res<()> {
    server.save_text_file(&path, &text)
}

#[tauri::command]
pub(crate) fn set_thread_role(server: Srv, id: String, role: Option<String>) -> Res<()> {
    server.set_thread_role(&id, role)
}

#[tauri::command]
pub(crate) fn set_thread_plan_mode(server: Srv, id: String, on: bool) -> Res<()> {
    server.set_thread_plan_mode(&id, on)
}

#[tauri::command]
pub(crate) fn set_thread_effort(server: Srv, id: String, effort: Option<String>) -> Res<()> {
    server.set_thread_effort(&id, effort)
}

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
pub(crate) fn attachment_for_path(path: String) -> Res<Attachment> {
    Attachment::from_path(&path)
}

#[tauri::command]
pub(crate) fn save_pasted_image(
    server: Srv,
    mime_type: String,
    data: String,
    name: Option<String>,
) -> Res<Attachment> {
    server.save_pasted_image(&mime_type, &data, name.as_deref())
}

#[tauri::command]
pub(crate) fn save_pasted_text(server: Srv, text: String) -> Res<Attachment> {
    server.save_pasted_text(&text)
}

/// The `vscode-editor-data` of the clipboard's copy, which holds its language when it came
/// from VS Code or another Electron editor. Runs off the main thread, since reading the
/// clipboard waits on the app that owns it.
#[tauri::command(async)]
pub(crate) fn clipboard_editor_data() -> Option<String> {
    clipboard::editor_data()
}

/// Saves the system clipboard's image as an attachment, for a paste the webview gave no file.
/// Returns None when the clipboard holds no image.
#[tauri::command]
pub(crate) async fn paste_clipboard_image(server: Srv<'_>) -> Res<Option<Attachment>> {
    let server = server.inner().clone();
    blocking(move || {
        clipboard::image()
            .map(|png| {
                let name = clipboard::image_name();
                server.save_pasted_image_bytes("image/png", &png, name.as_deref())
            })
            .transpose()
    })
    .await?
}

/// The paths of files and folders copied in a file manager, to attach like dropped ones.
#[tauri::command]
pub(crate) async fn clipboard_files() -> Res<Vec<String>> {
    blocking(clipboard::files).await
}

/// Copies `text` when the webview's own clipboard refused it.
#[tauri::command]
pub(crate) async fn copy_text(text: String) -> Res<()> {
    tauri::async_runtime::spawn_blocking(move || clipboard::write_text(text))
        .await
        .map_err(|e| e.to_string())?
}

/// Lets the webview load an image for its thumbnail. The asset scope is not kept across restarts,
/// so this runs again whenever a thumbnail is shown.
#[tauri::command]
pub(crate) fn allow_preview(app: tauri::AppHandle, path: String) -> Res<()> {
    app.asset_protocol_scope()
        .allow_file(&path)
        .map_err(|e| e.to_string())
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
    rule: Option<patchwork_server::types::RuleRef>,
) -> Res<()> {
    server.respond_approval(
        &thread_id,
        &approval_id,
        allow,
        answers.unwrap_or_default(),
        rule,
    )
}

#[tauri::command]
pub(crate) async fn scan_imports(server: Srv<'_>, kind: ImportSource) -> Res<ImportScan> {
    let server = server.inner().clone();
    blocking(move || server.scan_imports(kind)).await
}

#[tauri::command]
pub(crate) async fn import_threads(server: Srv<'_>, kinds: Vec<ImportSource>) -> Res<u32> {
    let server = server.inner().clone();
    blocking(move || server.import_threads(&kinds)).await?
}
