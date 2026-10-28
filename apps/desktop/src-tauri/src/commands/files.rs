//! Files and git in a project: reading, diffing, accepting, and reverting them.

use super::*;

#[tauri::command]
pub(crate) async fn missing_files(
    server: Srv<'_>,
    project_id: String,
    paths: Vec<String>,
) -> Res<Vec<String>> {
    let root = server.project_root(&project_id)?;
    blocking(move || workspace::missing_files(&root, &paths)).await
}

/// What each of a reply's files is shown as: a thumbnail, a card, or a line in the diff list.
#[tauri::command]
pub(crate) async fn file_media(
    server: Srv<'_>,
    project_id: String,
    paths: Vec<String>,
) -> Res<Vec<workspace::FileMedia>> {
    let root = server.project_root(&project_id)?;
    blocking(move || workspace::file_media(&root, &paths))
        .await?
        .map_err(String::from)
}

/// Opens a file inside the project in the user's default app.
#[tauri::command]
pub(crate) async fn open_file(
    app: AppHandle,
    server: Srv<'_>,
    project_id: String,
    path: String,
) -> Res<()> {
    let root = server.project_root(&project_id)?;
    let file = blocking(move || workspace::resolve_inside(&root, &path)).await??;
    blocking(move || open_in_viewer(&app, &file)).await?
}

/// What a reply's turn did to each of `paths`, and where each file stands now.
#[tauri::command]
pub(crate) async fn turn_files(
    server: Srv<'_>,
    project_id: String,
    turn: TurnTrees,
    paths: Vec<String>,
) -> Res<Vec<orchestration::checkpoint::TurnFile>> {
    let root = server.project_root(&project_id)?;
    blocking(move || orchestration::checkpoint::turn_files(&root, &turn, &paths)).await?
}

/// A reply's turn's diff of one file.
#[tauri::command]
pub(crate) async fn turn_file_diff(
    server: Srv<'_>,
    project_id: String,
    turn: TurnTrees,
    path: String,
) -> Res<String> {
    let root = server.project_root(&project_id)?;
    blocking(move || orchestration::checkpoint::turn_file_diff(&root, &turn, &path)).await?
}

#[tauri::command]
pub(crate) async fn read_file(server: Srv<'_>, project_id: String, path: String) -> Res<String> {
    let root = server.project_root(&project_id)?;
    blocking(move || workspace::read_file(&root, &path))
        .await?
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn git_branch(server: Srv<'_>, project_id: String) -> Res<Option<String>> {
    let root = server.project_root(&project_id)?;
    blocking(move || workspace::git_branch(&root)).await
}

#[tauri::command]
pub(crate) async fn git_changes(server: Srv<'_>, project_id: String) -> Res<Vec<FileChange>> {
    let root = server.project_root(&project_id)?;
    blocking(move || workspace::git_changes(&root))
        .await?
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn git_file_diff(
    server: Srv<'_>,
    project_id: String,
    path: String,
) -> Res<String> {
    let root = server.project_root(&project_id)?;
    blocking(move || workspace::git_file_diff(&root, &path))
        .await?
        .map_err(String::from)
}

/// Watches the active project in place of any other, emitting `project-changed` after each
/// settled burst of changes.
#[tauri::command]
pub(crate) fn watch_project(app: AppHandle, server: Srv, project_id: String) -> Res<()> {
    server.watch_project(&project_id, move |changed| {
        let _ = app.emit("project-changed", changed);
    })
}

#[tauri::command]
pub(crate) fn accept_file(server: Srv, project_id: String, path: String) -> Res<()> {
    workspace::accept_file(&server.project_root(&project_id)?, &path).map_err(String::from)
}

#[tauri::command]
pub(crate) fn revert_file(server: Srv, project_id: String, path: String) -> Res<()> {
    workspace::revert_file(&server.project_root(&project_id)?, &path).map_err(String::from)
}

#[tauri::command]
pub(crate) fn open_in_editor(server: Srv, project_id: String, path: String) -> Res<()> {
    workspace::open_in_editor(&server.project_root(&project_id)?, &path).map_err(String::from)
}

/// Shows a file selected in the system file manager. A relative path is taken from the project's
/// root; an absolute one is used as is.
#[tauri::command]
pub(crate) async fn reveal_file(
    app: AppHandle,
    server: Srv<'_>,
    project_id: Option<String>,
    path: String,
) -> Res<()> {
    use tauri_plugin_opener::OpenerExt;
    let path = match project_id {
        Some(id) => std::path::Path::new(&server.project_root(&id)?).join(&path),
        None => std::path::PathBuf::from(&path),
    };
    if !path.exists() {
        return Err(format!("{} no longer exists", path.display()));
    }
    blocking(move || {
        app.opener()
            .reveal_item_in_dir(path)
            .map_err(|e| e.to_string())
    })
    .await?
}

/// Opens a web or mail link in the user's default app. On Linux `xdg-open` runs with the user's
/// own environment, where the opener plugin would pass on the AppImage's.
#[tauri::command]
#[cfg_attr(target_os = "linux", allow(unused_variables))]
pub(crate) fn open_url(app: AppHandle, url: String) -> Res<()> {
    let scheme = url
        .split_once(':')
        .map(|(scheme, _)| scheme.to_ascii_lowercase());
    if !matches!(scheme.as_deref(), Some("http" | "https" | "mailto" | "tel")) {
        return Err("Patchwork only opens web and mail links".into());
    }
    #[cfg(target_os = "linux")]
    {
        let mut child = patchwork_server::host_command("xdg-open")
            .arg(&url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("Could not open the link: {e}"))?;
        std::thread::spawn(move || child.wait());
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        use tauri_plugin_opener::OpenerExt;
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|e| e.to_string())
    }
}
