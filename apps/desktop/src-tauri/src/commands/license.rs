//! The license key that unlocks a paid plan.

use super::*;

#[tauri::command]
pub(crate) async fn license_status(server: Srv<'_>) -> Res<LicenseStatus> {
    Ok(server.license_status())
}

#[tauri::command]
pub(crate) async fn set_license_key(server: Srv<'_>, key: String) -> Res<LicenseStatus> {
    server.set_license_key(&key)
}

#[tauri::command]
pub(crate) async fn remove_license_key(server: Srv<'_>) -> Res<LicenseStatus> {
    server.remove_license_key()
}

/// Asks the account service for the account's free trial, so it runs off the main thread.
#[tauri::command]
pub(crate) async fn start_trial(server: Srv<'_>) -> Res<LicenseStatus> {
    let server = server.inner().clone();
    blocking(move || server.start_trial()).await?
}
