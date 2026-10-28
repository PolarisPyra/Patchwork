//! Computer use: screenshots and input the agent may perform.

use super::*;

/// The screen as a PNG data URL. Behind Settings → Permissions: the command refuses when
/// computer use is off, so a stray call cannot look at the screen.
#[tauri::command]
pub(crate) async fn computer_screenshot(
    server: Srv<'_>,
) -> Res<patchwork_server::computer::Screenshot> {
    if !server.store.settings().computer_use {
        return Err("Computer use is off in Settings → Permissions".into());
    }
    blocking(patchwork_server::computer::screenshot).await?
}

/// Moves the pointer, clicks, types, presses a key, or scrolls.
#[tauri::command]
pub(crate) async fn computer_action(
    server: Srv<'_>,
    action: patchwork_server::computer::Action,
) -> Res<()> {
    if !server.store.settings().computer_use {
        return Err("Computer use is off in Settings → Permissions".into());
    }
    blocking(move || patchwork_server::computer::perform(&action)).await?
}
