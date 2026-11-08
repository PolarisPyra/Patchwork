//! API providers and the models agents offer: saving, removing, and fetching them.

use super::*;

/// Off the main thread: listing runs agent CLIs (`codex debug models`, `opencode models`), and
/// every new chat's composer asks for each agent's models, so the window stopped painting until
/// they answered.
#[tauri::command]
pub(crate) async fn list_models(kind: AgentKind) -> Res<Vec<ModelOption>> {
    blocking(move || provider::list_models(kind)).await
}

#[tauri::command]
pub(crate) async fn list_api_providers()
-> Res<Vec<patchwork_server::provider::api::config::ProviderView>> {
    blocking(patchwork_server::provider::api::config::list).await
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
pub(crate) async fn reveal_api_key(id: String) -> Res<Option<String>> {
    blocking(move || patchwork_server::provider::api::config::stored_key(&id)).await
}

#[tauri::command]
pub(crate) async fn fetch_api_models(
    query: patchwork_server::provider::api::config::ModelsQuery,
) -> Res<Vec<patchwork_server::provider::api::config::ApiModel>> {
    blocking(move || patchwork_server::provider::api::config::fetch_models(query)).await?
}

/// Probes a saved provider's endpoint for its live health, for Settings → Inference.
#[tauri::command]
pub(crate) async fn probe_api_provider(
    id: String,
) -> Res<patchwork_server::provider::api::config::ProviderHealth> {
    blocking(move || patchwork_server::provider::api::config::probe(&id)).await?
}

/// Scans well-known local ports for OpenAI-compatible model servers, for Find servers.
#[tauri::command]
pub(crate) async fn discover_inference_servers()
-> Res<Vec<patchwork_server::provider::api::config::DiscoveredServer>> {
    blocking(patchwork_server::provider::api::config::discover).await
}

/// Looks for where a saved server moved on its machine, and points the provider there.
#[tauri::command]
pub(crate) async fn relocate_api_provider(id: String) -> Res<String> {
    blocking(move || patchwork_server::provider::api::servers::relocate(&id)).await?
}

/// Adds the first local model server that answers, or reuses its saved provider, and returns one
/// of its models as `provider/model`.
#[tauri::command]
pub(crate) async fn use_local_model() -> Res<String> {
    blocking(patchwork_server::provider::api::servers::use_local_model).await?
}
