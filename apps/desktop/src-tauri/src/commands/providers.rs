//! API providers: saving, removing, and fetching their models.

use super::*;

#[tauri::command]
pub(crate) fn list_api_providers() -> Vec<provider::api_config::ProviderView> {
    provider::api_config::list()
}

#[tauri::command]
pub(crate) async fn save_api_provider(
    provider: provider::api_config::ApiProvider,
    key: provider::api_config::KeyChange,
) -> Res<provider::api_config::ProviderView> {
    blocking(move || provider::api_config::save(provider, key)).await?
}

#[tauri::command]
pub(crate) async fn remove_api_provider(id: String) -> Res<()> {
    blocking(move || provider::api_config::remove(&id)).await?
}
