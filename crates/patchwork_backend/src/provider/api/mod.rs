//! API providers as an agent: a turn runs Patchwork's own harness (`crate::harness`) against the
//! provider of the task's model, and this maps the harness's `api.*` lines.
//!
//! Store, vault-backed secrets, local discovery, and model listing live in
//! `patchwork-provider-api`.

// Rust guideline compliant 2026-11-23

mod agent;
pub(crate) use agent::Api;
mod turn;

#[doc(inline)]
pub use patchwork_provider_api::{
    ApiModel, ApiProvider, DiscoveredServer, Header, KeyChange, KeyStore, ModelsQuery,
    ProviderHealth, ProviderView, default_model, discover, fetch_models, init, key, list, models,
    move_to, probe, provider, providers, remove, resolve, save, servers, sessions_dir,
};
pub(crate) use patchwork_provider_api::{
    connection, delete_secret, forget_secrets, old_copies, read_secret, remove_old_copies,
    store_secret,
};
#[doc(inline)]
pub use turn::{
    approval_reply, auth, bridge_request, compact_window_line, interrupt, mcp_reply, parse,
    steer_line, turn_args,
};
