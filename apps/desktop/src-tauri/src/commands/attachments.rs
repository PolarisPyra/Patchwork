//! What a draft attaches from a drop or a paste, and the clipboard behind it.

use super::*;

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

/// The `vscode-editor-data` of the clipboard's copy, which holds its language when it came
/// from VS Code or another Electron editor. Runs off the main thread, since reading the
/// clipboard waits on the app that owns it.
#[tauri::command(async)]
pub(crate) fn clipboard_editor_data() -> Option<String> {
    clipboard::editor_data()
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
