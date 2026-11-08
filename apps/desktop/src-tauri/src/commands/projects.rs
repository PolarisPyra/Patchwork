//! Projects, the folders that group their threads, and what the sidebar remembers about them.

use super::*;

#[tauri::command]
pub(crate) fn add_project(server: Srv, path: String) -> Res<Project> {
    server.add_project(&path)
}

#[tauri::command]
pub(crate) fn create_project(server: Srv, parent: String, name: String) -> Res<Project> {
    server.create_project(&parent, &name)
}

#[tauri::command]
pub(crate) fn rename_project(server: Srv, id: String, name: String) -> Res<()> {
    server.rename_project(&id, &name)
}

#[tauri::command]
pub(crate) fn relocate_project(server: Srv, id: String, path: String) -> Res<()> {
    server.relocate_project(&id, &path)
}

#[tauri::command]
pub(crate) fn is_folder(path: String) -> bool {
    std::path::Path::new(&path).is_dir()
}

/// The machines the Add Project dialog offers: this computer, then the SSH hosts in `~/.ssh/config`.
#[tauri::command]
pub(crate) fn list_machines() -> Vec<patchwork_server::system::browse::Machine> {
    patchwork_server::system::browse::machines()
}

/// The sub-folders of `path` (this computer's home folder when none is given), for the folder browser.
#[tauri::command]
pub(crate) fn browse_dir(
    path: Option<String>,
) -> Res<patchwork_server::system::browse::DirListing> {
    patchwork_server::system::browse::browse(path)
}

/// The hosts in `~/.ssh/config`, for Settings → SSH.
#[tauri::command]
pub(crate) async fn list_ssh_hosts() -> Res<Vec<patchwork_server::system::ssh::SshHost>> {
    blocking(patchwork_server::system::ssh::hosts).await
}

/// Logs in to a configured host with keys only, to show whether it answers.
#[tauri::command]
pub(crate) async fn test_ssh_host(name: String) -> Res<patchwork_server::system::ssh::SshTest> {
    blocking(move || patchwork_server::system::ssh::test(&name)).await?
}

/// Opens `~/.ssh/config` in the user's editor. Patchwork never creates or writes it.
#[tauri::command]
pub(crate) async fn open_ssh_config(app: AppHandle) -> Res<()> {
    let path = patchwork_server::system::ssh::config_path().ok_or("No home folder")?;
    if !path.is_file() {
        return Err(format!("{} does not exist", path.display()));
    }
    let dir = path
        .parent()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default();
    edit_file(&app, dir, "config".to_string()).await
}

/// A worktree project removes its worktree too, which runs git.
#[tauri::command]
pub(crate) async fn remove_project(server: Srv<'_>, id: String) -> Res<()> {
    let server = server.inner().clone();
    blocking(move || server.remove_project(&id)).await?
}

/// Starts a git worktree of the project as a project of its own.
#[tauri::command]
pub(crate) async fn create_worktree(server: Srv<'_>, project_id: String) -> Res<Project> {
    let server = server.inner().clone();
    blocking(move || server.create_worktree(&project_id)).await?
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
pub(crate) fn get_collapsed(server: Srv) -> Vec<String> {
    server.collapsed_sections()
}

#[tauri::command]
pub(crate) fn set_collapsed(server: Srv, ids: Vec<String>) -> Res<()> {
    server.set_collapsed_sections(&ids)
}

#[tauri::command]
pub(crate) fn list_files(server: Srv, project_id: String) -> Res<Vec<String>> {
    server.list_files(&project_id)
}

/// Watches the active project in place of any other, emitting `project-changed` after each
/// settled burst of changes.
#[tauri::command]
pub(crate) fn watch_project(app: AppHandle, server: Srv, project_id: String) -> Res<()> {
    server.watch_project(&project_id, move |changed| {
        let _ = app.emit("project-changed", changed);
    })
}
