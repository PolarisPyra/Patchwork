//! The app's own updates.

use super::*;

#[tauri::command]
pub(crate) fn update_status(
    app: AppHandle,
    updates: State<updates::Updates>,
) -> updates::UpdateStatus {
    updates.status(&app)
}

/// Starts a check. Its progress and result arrive on `"update-status"`.
#[tauri::command]
pub(crate) fn check_for_updates(app: AppHandle) {
    std::thread::spawn(move || app.state::<updates::Updates>().check(&app, true));
}

#[tauri::command]
pub(crate) fn restart_to_update(
    app: AppHandle,
    updates: State<updates::Updates>,
    force: bool,
) -> updates::RestartResult {
    updates.restart(&app, force)
}
