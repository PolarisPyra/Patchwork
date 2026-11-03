//! The logs: their folder, reading and exporting them, and the window's own lines.

use super::*;

/// Opens the folder of the debug logs in the file manager, creating it first.
#[tauri::command]
pub(crate) async fn open_logs_folder(app: AppHandle) -> Res<()> {
    let dir = patchwork_server::system::dirs().logs.clone();
    open_folder(&app, &dir).await
}

/// This launch's logs, for the Output panel.
#[tauri::command]
pub(crate) fn log_channels() -> Vec<String> {
    patchwork_server::logging::channels()
}

/// A log from `offset` on, for the Output panel to follow.
#[tauri::command]
pub(crate) async fn read_log(
    channel: String,
    offset: u64,
) -> Res<patchwork_server::logging::LogChunk> {
    blocking(move || patchwork_server::logging::read(&channel, offset)).await?
}

/// A message from the window for `window/renderer.log`, and for the log of the chat it is about.
#[tauri::command]
pub(crate) fn log_renderer(
    level: patchwork_server::settings::LogLevel,
    message: String,
    thread_id: Option<String>,
) {
    use patchwork_server::logging::{Data, WINDOW_LOG, Source};
    let source = Source {
        thread: thread_id,
        ..Source::default()
    };
    WINDOW_LOG.write(level, "", &source, Data::Text(message));
}

/// Writes the report of a chat (see `logging::report`) and shows it selected in the file manager,
/// in front, among the other logs.
#[tauri::command]
pub(crate) async fn open_thread_logs(
    app: AppHandle,
    server: Srv<'_>,
    thread_id: String,
) -> Res<()> {
    let server = server.inner().clone();
    let path = blocking(move || server.thread_report(&thread_id)).await??;
    let token = activation::token(&app).await;
    blocking(move || reveal_in_file_manager(&app, &path, token.as_deref())).await?
}

/// Saves the kept sessions' logs as a zip at `path`, with `system.json`: what Patchwork runs on
/// (see `logging::system`) and the settings, secrets removed.
#[tauri::command]
pub(crate) async fn export_logs(server: Srv<'_>, path: String) -> Res<()> {
    let server = server.inner().clone();
    blocking(move || {
        let mut system = patchwork_server::logging::system::report().clone();
        system["settings"] = serde_json::json!(server.store.settings());
        patchwork_server::logging::export(std::path::Path::new(&path), &system)
    })
    .await?
}
