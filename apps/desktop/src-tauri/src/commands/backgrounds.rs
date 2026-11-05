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

/// Copies a picked picture into Patchwork's folder and returns the copy's file name.
#[tauri::command]
pub(crate) async fn import_background_image(server: Srv<'_>, path: String) -> Res<String> {
    let server = server.inner().clone();
    blocking(move || server.import_background_image(&path)).await?
}

/// Saves the picture on the system clipboard as a background and returns its file name, or None
/// when the clipboard holds no picture.
#[tauri::command]
pub(crate) async fn paste_background_image(server: Srv<'_>) -> Res<Option<String>> {
    let server = server.inner().clone();
    blocking(move || {
        clipboard::image()
            .map(|png| {
                let name = clipboard::image_name();
                let stem = name
                    .as_deref()
                    .and_then(|n| std::path::Path::new(n).file_stem())
                    .and_then(|n| n.to_str());
                server.import_background_bytes(png, stem)
            })
            .transpose()
    })
    .await?
}

#[tauri::command]
pub(crate) async fn list_background_images(server: Srv<'_>) -> Res<Vec<BackgroundImage>> {
    let server = server.inner().clone();
    blocking(move || server.list_background_images()).await?
}

#[tauri::command]
pub(crate) async fn remove_background_image(
    server: Srv<'_>,
    file: String,
) -> Res<RemovedBackground> {
    let server = server.inner().clone();
    blocking(move || server.remove_background_image(&file)).await?
}

#[tauri::command]
pub(crate) async fn restore_background_image(
    server: Srv<'_>,
    removed: RemovedBackground,
) -> Res<()> {
    let server = server.inner().clone();
    blocking(move || server.restore_background_image(&removed)).await?
}
