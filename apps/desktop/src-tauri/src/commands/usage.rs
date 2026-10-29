//! Subscription usage limits.

use super::*;

#[tauri::command]
/// Cached; `refresh` asks for new numbers, which the server still throttles.
pub(crate) async fn get_usage(
    server: Srv<'_>,
    kind: AgentKind,
    refresh: bool,
) -> Res<ProviderUsage> {
    let server = server.inner().clone();
    blocking(move || server.usage.get(kind, refresh)).await
}
