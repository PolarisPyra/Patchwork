//! API providers: OpenAI-compatible endpoints the user adds with their own key. The providers
//! live in `providers.json` in the config folder, which holds no secrets. Each key goes to
//! the system credential store (Keychain, Credential Manager, or the Secret Service), or, where
//! none is available, to `api-keys.json` beside it, readable only by the user. A provider can
//! instead name an environment variable that holds its key. Header values are stored the same
//! way, since one may carry a secret (an `Authorization` or `X-Api-Key` header), and the config
//! keeps only their names.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ts_rs::TS;

use crate::system::fs::write_private;
use crate::provider::ModelOption;

/// The credential store's service name for every key Patchwork saves.
const SERVICE: &str = "dev.patchwork.ade";
const CONFIG_FILE: &str = "providers.json";
const KEYS_FILE: &str = "api-keys.json";
/// Conversations of API tasks, one file per session, in the data folder.
const SESSIONS_DIR: &str = "api-sessions";

static DIR: OnceLock<PathBuf> = OnceLock::new();
/// Keys read this run, so a turn does not ask the credential store each time.
static KEYS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
/// Serializes changes to the config and keys files.
static WRITE: Mutex<()> = Mutex::new(());

/// Sets the config folder; called once when the server opens.
pub fn init(config_dir: &Path) {
    let _ = DIR.set(config_dir.to_path_buf());
}

fn dir() -> PathBuf {
    DIR.get().cloned().unwrap_or_default()
}

pub fn sessions_dir() -> PathBuf {
    crate::system::dirs().data.join(SESSIONS_DIR)
}

fn keys() -> std::sync::MutexGuard<'static, HashMap<String, String>> {
    KEYS.get_or_init(Default::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "provider.ts")]
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "provider.ts")]
#[serde(rename_all = "camelCase")]
pub struct ApiModel {
    pub id: String,
    /// Shown instead of the id, when set.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "provider.ts")]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "provider.ts")]
#[serde(rename_all = "lowercase")]
pub enum KeyStore {
    /// The system credential store.
    Keychain,
    /// `api-keys.json`, for systems without a credential store.
    File,
}

#[derive(Default, Serialize, Deserialize)]
struct Config {
    #[serde(default)]
    providers: Vec<ApiProvider>,
}

fn read_config() -> Config {
    crate::system::fs::read_json_or_default(&dir().join(CONFIG_FILE))
}

fn write_config(config: &Config) -> Result<(), String> {
    let text = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    write_private(&dir().join(CONFIG_FILE), &text)
}

pub fn providers() -> Vec<ApiProvider> {
    read_config().providers
}

pub fn provider(id: &str) -> Option<ApiProvider> {
    providers().into_iter().find(|p| p.id == id)
}

/// A provider as the settings show it: without its key, but saying whether it has one.
#[derive(Serialize, TS)]
#[ts(export, export_to = "provider.ts")]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    #[serde(flatten)]
    pub provider: ApiProvider,
    pub has_key: bool,
}

pub fn list() -> Vec<ProviderView> {
    providers()
        .into_iter()
        .map(with_headers)
        .map(|provider| ProviderView {
            has_key: provider.key_store.is_some() || provider.key_env.is_some(),
            provider,
        })
        .collect()
}

/// A new key for a provider being saved.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", tag = "action")]
pub enum KeyChange {
    Keep,
    Set { key: String },
    Clear,
}

/// Adds or replaces a provider. A new provider gets an id from its name.
pub fn save(mut provider: ApiProvider, key: KeyChange) -> Result<ProviderView, String> {
    provider.name = provider.name.trim().to_string();
    provider.base_url = provider.base_url.trim().trim_end_matches('/').to_string();
    if provider.name.is_empty() {
        return Err("Give the provider a name".into());
    }
    check_url(&provider.base_url)?;
    provider.key_env = provider
        .key_env
        .map(|v| v.trim().trim_start_matches('$').to_string())
        .filter(|v| !v.is_empty());
    provider.headers.retain(|h| !h.name.trim().is_empty());
    let values: Vec<String> = provider
        .headers
        .iter()
        .map(|h| h.value.trim().to_string())
        .collect();
    for h in &mut provider.headers {
        h.name = h.name.trim().to_string();
        h.value.clear();
    }
    provider.logo = provider.logo.filter(|l| !l.is_empty());
    if let Some(logo) = &provider.logo
        && (!logo.starts_with("data:image/") || logo.len() > 256 * 1024)
    {
        return Err("The logo must be an image under 256 KB".into());
    }
    let mut seen = std::collections::HashSet::new();
    provider
        .models
        .retain(|m| !m.id.trim().is_empty() && seen.insert(m.id.clone()));

    let _guard = WRITE.lock().unwrap_or_else(PoisonError::into_inner);
    let mut config = read_config();
    let existing = config.providers.iter().position(|p| p.id == provider.id);
    if existing.is_none() {
        provider.id = unique_id(&provider.name, &config.providers);
    }
    provider.key_store = existing.and_then(|i| config.providers[i].key_store);
    match key {
        KeyChange::Keep => {}
        KeyChange::Clear => {
            delete_secret(&provider.id);
            provider.key_store = None;
        }
        KeyChange::Set { key } if key.trim().is_empty() => {}
        KeyChange::Set { key } => {
            provider.key_store = Some(store_secret(&provider.id, key.trim())?);
        }
    }
    let headers_name = headers_secret(&provider.id);
    if values.iter().all(String::is_empty) {
        delete_secret(&headers_name);
    } else {
        store_secret(&headers_name, &json!(values).to_string())?;
    }
    match existing {
        Some(i) => config.providers[i] = provider.clone(),
        None => config.providers.push(provider.clone()),
    }
    write_config(&config)?;
    for (h, value) in provider.headers.iter_mut().zip(values) {
        h.value = value;
    }
    Ok(ProviderView {
        has_key: provider.key_store.is_some() || provider.key_env.is_some(),
        provider,
    })
}

pub fn remove(id: &str) -> Result<(), String> {
    let _guard = WRITE.lock().unwrap_or_else(PoisonError::into_inner);
    let mut config = read_config();
    config.providers.retain(|p| p.id != id);
    delete_secret(id);
    delete_secret(&headers_secret(id));
    write_config(&config)
}

fn unique_id(name: &str, taken: &[ApiProvider]) -> String {
    let base: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let base = if base.is_empty() {
        "provider".to_string()
    } else {
        base
    };
    let free = |id: &str| !taken.iter().any(|p| p.id == id);
    if free(&base) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|id| free(id))
        .unwrap_or(base)
}

/// A secret's entry in the credential store. A provider's key is named by the provider's id,
/// and its header values by `headers_secret`.
fn entry(name: &str) -> keyring::Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, &format!("api-provider:{name}"))
}

fn headers_secret(id: &str) -> String {
    format!("{id}:headers")
}

/// Saves a secret in the credential store, or in the keys file when there is none.
pub(crate) fn store_secret(name: &str, value: &str) -> Result<KeyStore, String> {
    keys().insert(name.to_string(), value.to_string());
    if entry(name).and_then(|e| e.set_password(value)).is_ok() {
        set_file_key(name, None)?;
        return Ok(KeyStore::Keychain);
    }
    set_file_key(name, Some(value))?;
    Ok(KeyStore::File)
}

pub(crate) fn delete_secret(name: &str) {
    keys().remove(name);
    if let Ok(entry) = entry(name) {
        let _ = entry.delete_credential();
    }
    if let Err(e) = set_file_key(name, None) {
        log::warn!("failed to remove an API secret from {KEYS_FILE}: {e}");
    }
}

/// The provider with its header values filled in from where they are stored.
fn with_headers(mut provider: ApiProvider) -> ApiProvider {
    // A config written before header values moved out still holds them.
    if provider.headers.iter().all(|h| !h.value.is_empty()) {
        return provider;
    }
    let values: Vec<String> = read_secret(&headers_secret(&provider.id))
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    for (h, value) in provider.headers.iter_mut().zip(values) {
        if h.value.is_empty() {
            h.value = value;
        }
    }
    provider
}

/// A secret saved with `store_secret`, from wherever it went.
pub(crate) fn read_secret(name: &str) -> Option<String> {
    if let Some(value) = keys().get(name) {
        return Some(value.clone());
    }
    let value = entry(name)
        .and_then(|e| e.get_password())
        .ok()
        .or_else(|| read_file_keys().remove(name))?;
    keys().insert(name.into(), value.clone());
    Some(value)
}

/// Accepts `http://` and `https://` base URLs.
fn check_url(url: &str) -> Result<(), String> {
    if url.starts_with("https://") || url.starts_with("http://") {
        Ok(())
    } else {
        Err("The base URL must start with http:// or https://".into())
    }
}

fn read_file_keys() -> HashMap<String, String> {
    std::fs::read_to_string(dir().join(KEYS_FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn set_file_key(id: &str, key: Option<&str>) -> Result<(), String> {
    let path = dir().join(KEYS_FILE);
    let mut file_keys = read_file_keys();
    let changed = match key {
        Some(key) => file_keys.insert(id.into(), key.into()).as_deref() != Some(key),
        None => file_keys.remove(id).is_some(),
    };
    if !changed {
        return Ok(());
    }
    if file_keys.is_empty() {
        return std::fs::remove_file(&path).or_else(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Ok(()),
            _ => Err(e.to_string()),
        });
    }
    let text = serde_json::to_string_pretty(&file_keys).map_err(|e| e.to_string())?;
    write_private(&path, &text)
}

/// The provider's key: from its environment variable, else where Patchwork stored it.
pub fn key(provider: &ApiProvider) -> Option<String> {
    if let Some(var) = &provider.key_env {
        return std::env::var(var)
            .ok()
            .filter(|v| !v.is_empty())
            .or_else(|| crate::system::login_var(var));
    }
    if let Some(key) = keys().get(&provider.id) {
        return Some(key.clone());
    }
    let key = match provider.key_store? {
        KeyStore::Keychain => entry(&provider.id).and_then(|e| e.get_password()).ok(),
        KeyStore::File => read_file_keys().remove(&provider.id),
    }?;
    keys().insert(provider.id.clone(), key.clone());
    Some(key)
}

/// Models of every enabled provider, as `provider/model` ids.
pub fn models() -> Vec<ModelOption> {
    providers()
        .into_iter()
        .filter(|p| p.enabled)
        .flat_map(|p| {
            p.models.into_iter().map(move |m| ModelOption {
                id: Some(format!("{}/{}", p.id, m.id)),
                label: m.name.clone().unwrap_or_else(|| m.id.clone()),
                default_model: None,
                is_legacy: false,
                efforts: None,
                context_window: None,
            })
        })
        .collect()
}

/// The first model of the first enabled provider, which a task with no model uses.
pub fn default_model() -> Option<String> {
    models().into_iter().next().and_then(|m| m.id)
}

/// Splits `provider/model` into the provider and the model's own id.
pub fn resolve(model: &str) -> Option<(ApiProvider, String)> {
    let (id, model) = model.split_once('/')?;
    Some((provider(id)?, model.to_string()))
}

/// What the models request needs: either a saved provider, or the settings being edited.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsQuery {
    pub base_url: String,
    #[serde(default)]
    pub headers: Vec<Header>,
    /// A key typed but not saved yet.
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub key_env: Option<String>,
    /// The saved provider whose key to use when none was typed.
    #[serde(default)]
    pub provider_id: Option<String>,
}

/// Asks the endpoint for its models (`GET /models`), which also checks the URL and key.
pub fn fetch_models(query: ModelsQuery) -> Result<Vec<ApiModel>, String> {
    let key = query
        .key
        .filter(|k| !k.trim().is_empty())
        .map(|k| k.trim().to_string())
        .or_else(|| {
            let var = query.key_env.filter(|v| !v.trim().is_empty());
            let saved = query.provider_id.as_deref().and_then(provider);
            match (var, saved) {
                (Some(var), _) => key(&ApiProvider {
                    key_env: Some(var),
                    ..blank()
                }),
                (None, Some(saved)) => key(&ApiProvider {
                    key_env: None,
                    ..saved
                }),
                (None, None) => None,
            }
        });
    let base = query.base_url.trim().trim_end_matches('/');
    check_url(base)?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut request = agent.get(format!("{base}/models"));
    if let Some(key) = &key {
        request = request.header("Authorization", &format!("Bearer {key}"));
    }
    for h in &query.headers {
        request = request.header(h.name.trim(), h.value.trim());
    }
    let mut response = request
        .call()
        .map_err(|e| format!("Could not reach {base}: {e}"))?;
    let status = response.status();
    let text = response.body_mut().read_to_string().unwrap_or_default();
    let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(crate::harness::error_text(status.as_u16(), &body, &text));
    }
    let list = body["data"]
        .as_array()
        .or_else(|| body["models"].as_array());
    let Some(list) = list else {
        return Err(format!(
            "{base}/models returned no model list: {} Local servers such as LM Studio serve models under /v1; check the base URL.",
            crate::harness::error_text(0, &body, &text)
        ));
    };
    let mut models: Vec<ApiModel> = list
        .iter()
        .filter_map(|m| {
            let id = m["id"].as_str().or_else(|| m["name"].as_str())?.to_string();
            Some(ApiModel { id, name: None })
        })
        .collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(models)
}

fn blank() -> ApiProvider {
    ApiProvider {
        id: String::new(),
        name: String::new(),
        preset: custom(),
        base_url: String::new(),
        enabled: true,
        models: Vec::new(),
        headers: Vec::new(),
        key_env: None,
        key_store: None,
        logo: None,
        system_prompt: String::new(),
    }
}

/// The request line the API bridge reads first: everything a turn needs, key included, so
/// the key never appears in the bridge's arguments or environment.
pub fn connection(model: Option<&str>) -> Result<Value, String> {
    let model = model
        .map(String::from)
        .or_else(default_model)
        .ok_or("Add a model to an API provider in Settings → API Providers first")?;
    let (provider, model_id) =
        resolve(&model).ok_or_else(|| format!("No API provider has the model {model}"))?;
    if !provider.enabled {
        return Err(format!(
            "{} is turned off in Settings → API Providers",
            provider.name
        ));
    }
    check_url(&provider.base_url)?;
    let provider = with_headers(provider);
    let key = key(&provider);
    if key.is_none() && provider.key_env.is_some() {
        let var = provider.key_env.as_deref().unwrap_or_default();
        return Err(format!("{var} is not set, so {} has no key", provider.name));
    }
    let headers: serde_json::Map<String, Value> = provider
        .headers
        .iter()
        .map(|h| (h.name.trim().to_string(), json!(h.value.trim())))
        .collect();
    Ok(json!({
        "baseUrl": provider.base_url,
        "apiKey": key,
        "headers": headers,
        "model": model_id,
        "provider": provider.name,
        "preset": provider.preset,
        "systemPrompt": provider.system_prompt,
    }))
}
