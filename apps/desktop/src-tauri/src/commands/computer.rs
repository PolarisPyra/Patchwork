//! Computer use: screenshots and input the agent may perform.

use super::*;

/// The screen as a PNG data URL. Behind Settings → Permissions: the command refuses when
/// computer use is off, so a stray call cannot look at the screen.
#[tauri::command]
pub(crate) async fn computer_screenshot() -> Res<patchwork_server::computer::Screenshot> {
    patchwork_server::computer::ensure_enabled()?;
    blocking(patchwork_server::computer::screenshot).await?
}

/// Moves the pointer, clicks, types, presses a key, or scrolls.
#[tauri::command]
pub(crate) async fn computer_action(action: patchwork_server::computer::Action) -> Res<()> {
    patchwork_server::computer::ensure_enabled()?;
    blocking(move || patchwork_server::computer::perform(&action)).await?
}
