//! What a saved API provider is, and how the settings see and change one.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiProvider {
    /// A slug, the part of a model id before its `/`.
    pub id: String,
    pub name: String,
    /// The preset it was made from (`deepseek`, `openai`, ... or `custom`); picks its logo.
    #[serde(default = "custom")]
    pub preset: String,
    /// The OpenAI-compatible base URL, e.g. `https://api.deepseek.com`.
    pub base_url: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Found on this computer, added from Settings → Providers or by Use a local model: listed under
    /// Local providers there, with live health. Every other provider, added from a preset or by its
    /// address, is `false` and listed above them.
    pub local: bool,
    #[serde(default)]
    pub models: Vec<ApiModel>,
    /// Extra request headers, sent with every request.
    #[serde(default)]
    pub headers: Vec<Header>,
    /// Reads the key from this environment variable instead of a stored key.
    #[serde(default)]
    pub key_env: Option<String>,
    /// Where the stored key is; set by Patchwork, never by the UI.
    #[serde(default)]
    pub key_store: Option<KeyStore>,
    /// A logo chosen for it, as an image `data:` URL; replaces the preset's.
    #[serde(default)]
    pub logo: Option<String>,
    /// Added to the system prompt of every turn on this provider's models.
    #[serde(default)]
    pub system_prompt: String,
}

fn custom() -> String {
    "custom".into()
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiModel {
    pub id: String,
    /// Shown instead of the id, when set.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyStore {
    /// The system credential store.
    Keychain,
    /// `api-keys.json`, for systems without a credential store.
    File,
}

/// A provider as the settings show it: without its key, but saying whether it has one.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    #[serde(flatten)]
    pub provider: ApiProvider,
    pub has_key: bool,
}

/// A new key for a provider being saved.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", tag = "action")]
pub enum KeyChange {
    Keep,
    Set { key: String },
    Clear,
}

/// A provider with nothing set, to fill in the fields a request needs.
pub(super) fn blank() -> ApiProvider {
    ApiProvider {
        id: String::new(),
        name: String::new(),
        preset: custom(),
        base_url: String::new(),
        enabled: true,
        local: false,
        models: Vec::new(),
        headers: Vec::new(),
        key_env: None,
        key_store: None,
        logo: None,
        system_prompt: String::new(),
    }
}
