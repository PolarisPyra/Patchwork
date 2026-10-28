//! Plugins: the list, enabling, and installing them.

use super::*;

/// The plugin bundles in the plugins folder, with their hooks and apps.
#[tauri::command]
pub(crate) fn list_plugins(server: Srv) -> Vec<Plugin> {
    patchwork_server::plugins::list(&server.store.settings())
}

#[tauri::command]
pub(crate) async fn open_plugins_folder(app: AppHandle, server: Srv<'_>) -> Res<()> {
    open_folder(
        &app,
        &patchwork_server::plugins::dir(&server.store.settings()),
    )
    .await
}
