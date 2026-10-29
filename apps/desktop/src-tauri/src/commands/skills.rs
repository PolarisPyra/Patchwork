//! Skills and plugin bundles, and the folders they live in.

use super::*;

#[derive(serde::Serialize)]
pub(crate) struct SkillsView {
    dir: String,
    skills: Vec<patchwork_server::skills::Skill>,
}

/// The skills folder in use and the skills in it.
#[tauri::command]
pub(crate) fn list_skills(server: Srv) -> SkillsView {
    let dir = patchwork_server::skills::dir(&server.store.settings());
    SkillsView {
        skills: patchwork_server::skills::all(&server.store.settings()),
        dir: dir.to_string_lossy().into_owned(),
    }
}

/// Opens the skills folder in the file manager, creating it first.
#[tauri::command]
pub(crate) async fn open_skills_folder(app: AppHandle, server: Srv<'_>) -> Res<()> {
    open_folder(
        &app,
        &patchwork_server::skills::dir(&server.store.settings()),
    )
    .await
}

/// The skills, as `list_skills` gives them, and then again on `on_change` after each change to
/// the skills folder, until `unwatch_skills` or another watch. Not `async`, so a watch and the
/// unwatch before it run in the order they were called.
#[tauri::command]
pub(crate) fn watch_skills(server: Srv, on_change: Channel<SkillsView>) -> Res<SkillsView> {
    let view = list_skills(server.clone());
    let dir = view.dir.clone();
    server.watch_skills(move |skills| {
        let _ = on_change.send(SkillsView {
            dir: dir.clone(),
            skills,
        });
    })?;
    Ok(view)
}

#[tauri::command]
pub(crate) fn unwatch_skills(server: Srv) {
    server.unwatch_skills();
}

/// Opens a skill's file in the user's editor.
#[tauri::command]
pub(crate) async fn open_skill(app: AppHandle, server: Srv<'_>, path: String) -> Res<()> {
    if !patchwork_server::skills::all(&server.store.settings())
        .iter()
        .any(|s| s.path == path)
    {
        return Err("That is not a skill".into());
    }
    let path = std::path::PathBuf::from(path);
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Err("That is not a skill".into());
    };
    edit_file(
        &app,
        dir.to_string_lossy().into(),
        name.to_string_lossy().into(),
    )
    .await
}

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
