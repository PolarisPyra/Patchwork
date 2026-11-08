//! Git in a project: its branch, changes, diffs, and history, accepting or reverting a file, and
//! committing, pushing, and opening a pull request.

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

#[tauri::command]
pub(crate) async fn git_sync_state(server: Srv<'_>, project_id: String) -> Res<GitSyncState> {
    let root = server.project_root(&project_id)?;
    blocking(move || workspace::git_sync_state(&root)).await
}

#[tauri::command]
pub(crate) async fn git_log(
    server: Srv<'_>,
    project_id: String,
    skip: usize,
    limit: usize,
) -> Res<Vec<GitCommit>> {
    let server = server.inner().clone();
    blocking(move || server.git_log(&project_id, skip, limit)).await?
}

/// Stages everything and commits it; a blank message is written by a model. Returns the short
/// hash and the subject.
#[tauri::command]
pub(crate) async fn commit_changes(
    server: Srv<'_>,
    project_id: String,
    thread_id: Option<String>,
    message: String,
) -> Res<(String, String)> {
    let server = server.inner().clone();
    blocking(move || server.commit_changes(&project_id, thread_id.as_deref(), &message)).await?
}

/// Pushes the branch, setting its upstream on the first push. Returns where it went.
#[tauri::command]
pub(crate) async fn git_push(server: Srv<'_>, project_id: String) -> Res<String> {
    let root = server.project_root(&project_id)?;
    blocking(move || workspace::push(&root))
        .await?
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn open_pull_request(
    server: Srv<'_>,
    project_id: String,
    thread_id: Option<String>,
) -> Res<PullRequest> {
    let server = server.inner().clone();
    blocking(move || server.open_pull_request(&project_id, thread_id.as_deref())).await?
}

/// Stages everything and has a model write a commit message for it.
#[tauri::command]
pub(crate) async fn write_commit_message(
    server: Srv<'_>,
    project_id: String,
    thread_id: Option<String>,
) -> Res<String> {
    let server = server.inner().clone();
    blocking(move || server.write_commit_message(&project_id, thread_id.as_deref())).await?
}

/// A prompt with the open pull request's failed checks and comments, and the thread to send it to.
#[tauri::command]
pub(crate) async fn pull_request_feedback(
    server: Srv<'_>,
    project_id: String,
) -> Res<(Option<String>, String)> {
    let server = server.inner().clone();
    blocking(move || server.pull_request_feedback(&project_id)).await?
}

/// A prompt for an agent to resolve the project's merge conflicts.
#[tauri::command]
pub(crate) async fn conflict_prompt(server: Srv<'_>, project_id: String) -> Res<String> {
    let server = server.inner().clone();
    blocking(move || server.conflict_prompt(&project_id)).await?
}
