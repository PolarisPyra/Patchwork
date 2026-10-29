//! Git in a project: its branch, changes, and diffs, and accepting or reverting a file.

use super::*;

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

#[tauri::command]
pub(crate) fn accept_file(server: Srv, project_id: String, path: String) -> Res<()> {
    workspace::accept_file(&server.project_root(&project_id)?, &path).map_err(String::from)
}

#[tauri::command]
pub(crate) fn revert_file(server: Srv, project_id: String, path: String) -> Res<()> {
    workspace::revert_file(&server.project_root(&project_id)?, &path).map_err(String::from)
}
