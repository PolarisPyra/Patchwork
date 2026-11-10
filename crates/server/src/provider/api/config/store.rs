//! The saved providers in `providers.json`: reading, saving, moving and removing them.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::Error;
use crate::provider::api::servers;
use crate::system::fs::write_private;

use super::secrets::{delete_secret, headers_secret, store_secret, with_headers};
use super::types::{ApiProvider, KeyChange, ProviderView};

const CONFIG_FILE: &str = "providers.json";
/// Conversations of API tasks, one file per session, in the data folder.
const SESSIONS_DIR: &str = "api-sessions";

static DIR: OnceLock<PathBuf> = OnceLock::new();
/// Serializes changes to the config and keys files.
static WRITE: Mutex<()> = Mutex::new(());

/// Sets the config folder; called once when the server opens.
pub fn init(config_dir: &Path) {
    let _ = DIR.set(config_dir.to_path_buf());
}

pub(super) fn dir() -> PathBuf {
    DIR.get().cloned().unwrap_or_default()
}

#[must_use]
pub fn sessions_dir() -> PathBuf {
    crate::system::dirs().data.join(SESSIONS_DIR)
}

#[derive(Default, Serialize, Deserialize)]
struct Config {
    #[serde(default)]
    providers: Vec<ApiProvider>,
}

/// The saved providers. A provider saved before `local` existed is local when its URL points at
/// this computer, and the file is saved once with it.
fn read_config() -> Config {
    let path = dir().join(CONFIG_FILE);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Config::default();
    };
    let Ok(mut value) = serde_json::from_str::<Value>(&text) else {
        // Not JSON: this keeps a copy beside it and starts over, as for any settings file.
        return crate::system::fs::read_json_or_default(&path);
    };
    let mut filled = false;
    if let Some(list) = value.get_mut("providers").and_then(Value::as_array_mut) {
        for p in list.iter_mut().filter_map(Value::as_object_mut) {
            if p.contains_key("local") {
                continue;
            }
            let url = p.get("baseUrl").and_then(Value::as_str).unwrap_or_default();
            let local = servers::on_this_computer(url);
            log::info!(
                provider = p.get("id").and_then(Value::as_str).unwrap_or("?"),
                kind = if local { "local" } else { "cloud" };
                "API provider {{provider}} taken as {{kind}}"
            );
            p.insert("local".into(), Value::Bool(local));
            filled = true;
        }
    }
    match serde_json::from_value::<Config>(value) {
        Ok(config) => {
            if filled && let Err(e) = write_config(&config) {
                log::warn!(error:% = e; "Could not save where each API provider runs: {{error}}");
            }
            config
        }
        Err(_) => crate::system::fs::read_json_or_default(&path),
    }
}

fn write_config(config: &Config) -> Result<(), Error> {
    let text = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    write_private(&dir().join(CONFIG_FILE), &text)
}

#[must_use]
pub fn providers() -> Vec<ApiProvider> {
    read_config().providers
}

#[must_use]
pub fn provider(id: &str) -> Option<ApiProvider> {
    providers().into_iter().find(|p| p.id == id)
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

/// Adds or replaces a provider. A new provider gets an id from its name.
///
/// # Errors
///
/// Returns an error when the name is empty, when the base URL does not start with `http://` or
/// `https://`, when the logo is not an image under 256 KB, or when the key or the config file
/// cannot be saved.
pub fn save(mut provider: ApiProvider, key: KeyChange) -> Result<ProviderView, Error> {
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

/// Points a saved provider at a new base URL, keeping the rest of it.
///
/// # Errors
///
/// Returns an error when the base URL does not start with `http://` or `https://`, when the
/// provider is no longer saved, or when the config file cannot be written.
pub fn move_to(id: &str, base_url: &str) -> Result<(), Error> {
    let base_url = base_url.trim().trim_end_matches('/');
    check_url(base_url)?;
    let _guard = WRITE.lock().unwrap_or_else(PoisonError::into_inner);
    let mut config = read_config();
    let provider = config
        .providers
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or("That provider is no longer saved")?;
    provider.base_url = base_url.to_string();
    write_config(&config)
}

/// Removes the saved API provider `id`.
///
/// # Errors
///
/// Returns an error when the config file cannot be written.
pub fn remove(id: &str) -> Result<(), Error> {
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
    // One of `taken.len() + 1` numbers is always free.
    (2..=taken.len() + 2)
        .map(|n| format!("{base}-{n}"))
        .find(|id| free(id))
        .unwrap_or(base)
}

/// Accepts `http://` and `https://` base URLs.
pub(super) fn check_url(url: &str) -> Result<(), Error> {
    if url.starts_with("https://") || url.starts_with("http://") {
        Ok(())
    } else {
        Err("The base URL must start with http:// or https://".into())
    }
}
