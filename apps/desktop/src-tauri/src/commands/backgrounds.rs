//! Chat backgrounds: a chat's own, and the pictures picked for one.

use super::*;

#[tauri::command]
pub(crate) async fn set_thread_background(
    server: Srv<'_>,
    id: String,
    background: Option<ChatBackground>,
) -> Res<()> {
    server.set_thread_background(&id, background)
}

/// Copies a picked picture into Patchwork's folder and returns the copy's path.
#[tauri::command]
pub(crate) async fn import_background_image(server: Srv<'_>, path: String) -> Res<String> {
    let server = server.inner().clone();
    blocking(move || server.import_background_image(&path)).await?
}
