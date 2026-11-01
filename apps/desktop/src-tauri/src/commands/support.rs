//! What the command groups share: the server state, running work off the main thread, and opening
//! files and folders on the desktop.

use std::sync::Arc;

use crate::activation;

use patchwork_server::{Res, Server, workspace};
use tauri::{AppHandle, State};

pub(super) type Srv<'a> = State<'a, Arc<Server>>;

/// Runs blocking work (CLI probes, network, disk scans) off the main thread.
pub(super) async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())
}

pub(super) async fn open_folder(app: &AppHandle, dir: &std::path::Path) -> Res<()> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    open_in_viewer(app, dir).await
}

/// Opens a file or folder in the user's default app.
#[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
pub(super) async fn open_in_viewer(app: &AppHandle, path: &std::path::Path) -> Res<()> {
    let token = activation::token(app).await;
    #[cfg(target_os = "linux")]
    {
        patchwork_server::system::launch::xdg_open(path.as_os_str(), token.as_deref())
            .map_err(|e| format!("Could not open {}: {e}", path.display()))
    }
    #[cfg(not(target_os = "linux"))]
    {
        use tauri_plugin_opener::OpenerExt;
        app.opener()
            .open_path(path.to_string_lossy(), None::<&str>)
            .map_err(|e| e.to_string())
    }
}

/// Shows `path` selected in the file manager.
#[allow(unused_variables)]
pub(super) fn reveal_in_file_manager(
    app: &AppHandle,
    path: &std::path::Path,
    token: Option<&str>,
) -> Res<()> {
    #[cfg(target_os = "linux")]
    {
        patchwork_server::system::launch::reveal(path, token).map_err(String::from)
    }
    #[cfg(not(target_os = "linux"))]
    {
        use tauri_plugin_opener::OpenerExt;
        app.opener()
            .reveal_item_in_dir(path)
            .map_err(|e| e.to_string())
    }
}

/// Opens `name` in the folder `dir` in the user's editor.
pub(super) async fn edit_file(app: &AppHandle, dir: String, name: String) -> Res<()> {
    let token = activation::token(app).await;
    blocking(move || workspace::open_in_editor(&dir, &name, token.as_deref()))
        .await?
        .map_err(String::from)
}
