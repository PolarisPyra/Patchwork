//! API providers as an agent: a turn runs Patchwork's own harness (`crate::harness`) against the
//! provider of the task's model, and this maps the harness's `api.*` lines.
//!
//! Each provider is an OpenAI-compatible endpoint the user adds with their own key.
//!
//! The providers live in `providers.json` in the config folder, which holds no secrets. Each key
//! goes to the vault, and is never written to a file as it is: the `api-keys.json` an early
//! version wrote beside the config is moved into the vault and deleted. A provider can instead name
//! an environment variable that holds its key. Header values are stored the same way, since one
//! may carry a secret (an `Authorization` or `X-Api-Key` header), and the config keeps only their
//! names.

// Rust guideline compliant 2026-11-23

mod health;
mod models;
mod secrets;
pub mod servers;
mod store;
mod turn;
mod types;

#[doc(inline)]
pub use health::{DiscoveredServer, ProviderHealth, discover, probe};
pub(crate) use models::connection;
#[doc(inline)]
pub use models::{ModelsQuery, default_model, fetch_models, models, resolve};
pub(crate) use secrets::{
    delete_secret, forget_secrets, old_copies, read_secret, remove_old_copies, store_secret,
    take_keys_file,
};
#[doc(inline)]
pub use secrets::key;
#[doc(inline)]
pub use store::{init, list, move_to, provider, providers, remove, save, sessions_dir};
#[doc(inline)]
pub use types::{ApiModel, ApiProvider, Header, KeyChange, KeyStore, ProviderView};
#[doc(inline)]
pub use turn::{
    approval_reply, auth, bridge_request, compact_window_line, interrupt, mcp_reply, parse,
    steer_line, turn_args,
};
