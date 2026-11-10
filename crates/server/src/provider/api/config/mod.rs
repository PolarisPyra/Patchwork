//! API providers: OpenAI-compatible endpoints the user adds with their own key.
//!
//! The providers live in `providers.json` in the config folder, which holds no secrets. Each key
//! goes to the system credential store (Keychain, Credential Manager, or the Secret Service), or,
//! where none is available, to `api-keys.json` beside it, readable only by the user. A provider can
//! instead name an environment variable that holds its key. Header values are stored the same way,
//! since one may carry a secret (an `Authorization` or `X-Api-Key` header), and the config keeps
//! only their names.

// Rust guideline compliant 2026-10-09

mod health;
mod models;
mod secrets;
mod store;
mod types;

#[doc(inline)]
pub use health::{DiscoveredServer, ProviderHealth, discover, probe};
pub(crate) use models::connection;
#[doc(inline)]
pub use models::{ModelsQuery, default_model, fetch_models, models, resolve};
pub(super) use secrets::with_headers;
pub(crate) use secrets::{delete_secret, read_secret, store_secret};
#[doc(inline)]
pub use secrets::{key, stored_key};
#[doc(inline)]
pub use store::{init, list, move_to, provider, providers, remove, save, sessions_dir};
#[doc(inline)]
pub use types::{ApiModel, ApiProvider, Header, KeyChange, KeyStore, ProviderView};
