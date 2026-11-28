//! OpenAI-compatible API providers: catalog store, vault-backed secrets, local server discovery,
//! health checks, and model listing.

// Rust guideline compliant 2026-11-23

mod health;
mod models;
mod secrets;
pub mod servers;
mod store;

#[doc(inline)]
pub use patchwork_model::provider::api::{
    ApiModel, ApiProvider, Header, KeyChange, KeyStore, ProviderView,
};
#[doc(inline)]
pub use health::{DiscoveredServer, ProviderHealth, discover, probe};
#[doc(inline)]
pub use models::{ModelsQuery, connection, default_model, error_text, fetch_models, models, resolve};
#[doc(inline)]
pub use secrets::{
    delete_secret, forget_secrets, key, old_copies, read_secret, remove_old_copies, store_secret,
};
#[doc(inline)]
pub use store::{init, list, move_to, provider, providers, remove, save, sessions_dir};
