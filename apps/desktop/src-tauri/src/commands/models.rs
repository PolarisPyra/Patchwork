//! API providers and the models agents offer: saving, removing, and fetching them.

use super::*;

#[tauri::command]
pub(crate) fn list_models(kind: AgentKind) -> Vec<ModelOption> {
    provider::list_models(kind)
}

#[tauri::command]
pub(crate) fn list_api_providers() -> Vec<patchwork_server::provider::api::config::ProviderView> {
    patchwork_server::provider::api::config::list()
}

#[tauri::command]
pub(crate) async fn save_api_provider(
    provider: patchwork_server::provider::api::config::ApiProvider,
    key: patchwork_server::provider::api::config::KeyChange,
) -> Res<patchwork_server::provider::api::config::ProviderView> {
    blocking(move || patchwork_server::provider::api::config::save(provider, key)).await?
}

#[tauri::command]
pub(crate) async fn remove_api_provider(id: String) -> Res<()> {
    blocking(move || patchwork_server::provider::api::config::remove(&id)).await?
}

#[tauri::command]
pub(crate) async fn fetch_api_models(
    query: patchwork_server::provider::api::config::ModelsQuery,
) -> Res<Vec<patchwork_server::provider::api::config::ApiModel>> {
    blocking(move || patchwork_server::provider::api::config::fetch_models(query)).await?
}
