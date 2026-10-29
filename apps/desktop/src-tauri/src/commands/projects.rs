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

#[tauri::command]
pub(crate) fn remove_project(server: Srv, id: String) -> Res<()> {
    server.remove_project(&id)
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
