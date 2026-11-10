//! Where provider secrets are kept: the system credential store, or `api-keys.json` without one.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, PoisonError};

use crate::Error;
use crate::system::fs::write_private;

use super::store::{dir, provider};
use super::types::{ApiProvider, KeyStore};

/// The credential store's service name for every key Patchwork saves.
const SERVICE: &str = "dev.patchwork.ade";
const KEYS_FILE: &str = "api-keys.json";

/// Keys read this run, so a turn does not ask the credential store each time.
static KEYS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

fn keys() -> std::sync::MutexGuard<'static, HashMap<String, String>> {
    KEYS.get_or_init(Default::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// A secret's entry in the credential store. A provider's key is named by the provider's id,
/// and its header values by `headers_secret`.
fn entry(name: &str) -> keyring::Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, &format!("api-provider:{name}"))
}

pub(super) fn headers_secret(id: &str) -> String {
    format!("{id}:headers")
}

/// Saves a secret in the credential store, or in the keys file when there is none.
pub(crate) fn store_secret(name: &str, value: &str) -> Result<KeyStore, Error> {
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
        log::warn!(
            file = KEYS_FILE, error:% = e;
            "failed to remove an API secret from {{file}}: {{error}}"
        );
    }
}

/// The provider with its header values filled in from where they are stored.
pub(in crate::provider::api) fn with_headers(mut provider: ApiProvider) -> ApiProvider {
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

fn read_file_keys() -> HashMap<String, String> {
    std::fs::read_to_string(dir().join(KEYS_FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn set_file_key(id: &str, key: Option<&str>) -> Result<(), Error> {
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
            _ => Err(e.into()),
        });
    }
    let text = serde_json::to_string_pretty(&file_keys).map_err(|e| e.to_string())?;
    write_private(&path, &text)
}

/// The provider's key: from its environment variable, else where Patchwork stored it.
#[must_use]
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

/// The key Patchwork stored for a provider, for the settings to show when asked. An environment
/// variable's value is not returned.
#[must_use]
pub fn stored_key(id: &str) -> Option<String> {
    provider(id)
        .filter(|p| p.key_store.is_some())
        .and_then(|p| key(&p))
}
