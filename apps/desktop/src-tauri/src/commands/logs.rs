//! The logs: their folder, reading and exporting them, and the window's own lines.

use super::*;

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

/// A message from the window for `window/renderer.log`.
#[tauri::command]
pub(crate) fn log_renderer(level: patchwork_server::settings::LogLevel, message: String) {
    use patchwork_server::logging::{Data, RENDERER, Source};
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
        patchwork_server::logging::export(std::path::Path::new(&path), &system)
    })
    .await?
}
