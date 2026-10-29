//! Settings, and the permission rules and shells they offer.

use super::*;

#[tauri::command]
pub(crate) fn get_settings(server: Srv) -> Settings {
    server.store.settings()
}

#[tauri::command]
pub(crate) fn save_settings(app: AppHandle, server: Srv, settings: Settings) -> Res<()> {
    let logo = settings.logo.clone();
    let logo_changed = server.store.settings().logo != logo;
    server.save_settings(settings)?;
    if logo_changed {
        logo::apply(&app, logo.as_deref());
    }
    Ok(())
}

/// The rules Settings → Permissions starts with, which Restore defaults puts back.
#[tauri::command]
pub(crate) async fn default_permission_rules()
-> Res<Vec<patchwork_server::permissions::PermissionRule>> {
    Ok(patchwork_server::permissions::default_rules())
}

#[tauri::command]
pub(crate) fn list_shells() -> ShellChoices {
    ShellChoices {
        shells: patchwork_server::system::available_shells(),
        default: patchwork_server::system::effective_shell(None),
    }
}
