//! Where provider secrets are kept: the vault, and behind it the stores of earlier versions,
//! the system credential store and `api-keys.json`.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, PoisonError};

use crate::Error;
use crate::system::fs::write_private;
use crate::vault;

use super::store::dir;
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

/// Whether the old stores may be read for `name`. A debug build shares them with the released
/// app, and takes its API and MCP secrets from there, but never its chat app credentials: a
/// debug build's bots are its own.
fn imports(name: &str) -> bool {
    !(cfg!(debug_assertions) && name.starts_with("gateway:"))
}

/// The secret in the old stores: the credential store, then the keys file.
fn legacy(name: &str) -> Option<String> {
    entry(name)
        .and_then(|e| e.get_password())
        .ok()
        .or_else(|| read_file_keys().remove(name))
}

/// Removes the secret from the old stores.
fn delete_legacy(name: &str) {
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

/// Saves a secret in the vault, saying what keeps the vault's key. A process that is not the
/// app has no vault, and saves in the credential store, or in the keys file when there is none.
///
/// # Errors
///
/// Returns an error when the vault is locked or cannot be written, or the keys file cannot.
pub(crate) fn store_secret(name: &str, value: &str) -> Result<KeyStore, Error> {
    match vault::status() {
        vault::Status::Open(method) => {
            vault::put(name, value)?;
            keys().insert(name.to_string(), value.to_string());
            return Ok(match method {
                // What says where a key is kept reads the vault's status for a password.
                vault::Method::Keychain | vault::Method::Password => KeyStore::Keychain,
                vault::Method::File => KeyStore::File,
            });
        }
        vault::Status::Locked(_) | vault::Status::Waiting => {
            // Fails with why the vault is locked.
            vault::put(name, value)?;
        }
        vault::Status::Closed => {}
    }
    keys().insert(name.to_string(), value.to_string());
    if entry(name).and_then(|e| e.set_password(value)).is_ok() {
        // The secret is saved. A keys file that cannot be read keeps its stale copy, at worst.
        if let Err(e) = set_file_key(name, None) {
            log::warn!(error:% = e; "could not clear a secret from the keys file: {{error}}");
        }
        return Ok(KeyStore::Keychain);
    }
    set_file_key(name, Some(value))?;
    Ok(KeyStore::File)
}

pub(crate) fn delete_secret(name: &str) {
    keys().remove(name);
    let open = matches!(vault::status(), vault::Status::Open(_));
    // A debug build leaves the old stores to the released app. An empty secret in its vault
    // stands for the removal, so the old value is not taken in again.
    let removed = if open && cfg!(debug_assertions) && imports(name) {
        vault::put(name, "")
    } else {
        vault::remove(name)
    };
    if let Err(e) = removed {
        log::warn!(error:% = e; "failed to remove a secret from the vault: {{error}}");
    }
    if !cfg!(debug_assertions) || matches!(vault::status(), vault::Status::Closed) {
        delete_legacy(name);
    }
}

/// Forgets the secrets read this run, as the vault is locked.
pub(crate) fn forget_secrets() {
    keys().clear();
}

/// How many copies the old stores hold: one for each of `names` the credential store has an
/// entry for, and one for each key in the keys file.
pub(crate) fn old_copies(names: &[String]) -> usize {
    let kept = names
        .iter()
        .filter(|name| entry(name).and_then(|e| e.get_password()).is_ok())
        .count();
    kept + read_file_keys().len()
}

/// Removes the old stores' copies of `names`, which the vault holds, and the keys file, whose
/// keys the vault does not hold yet are moved into it first. Says how many copies went.
///
/// # Errors
///
/// Returns an error when the vault is not open, a key cannot be moved into it, or the keys
/// file cannot be read or removed.
pub(crate) fn remove_old_copies(names: &[String]) -> Result<usize, Error> {
    if !matches!(vault::status(), vault::Status::Open(_)) {
        return Err("Patchwork's vault is not open".into());
    }
    let file = file_keys()?;
    for (name, value) in &file {
        if !names.contains(name) && imports(name) {
            vault::put(name, value)?;
        }
    }
    let mut removed = 0;
    for name in names {
        if let Ok(entry) = entry(name)
            && entry.get_password().is_ok()
        {
            entry
                .delete_credential()
                .map_err(|e| format!("The system keychain kept an old copy: {e}"))?;
            removed += 1;
        }
    }
    if !file.is_empty() {
        std::fs::remove_file(dir().join(KEYS_FILE))
            .map_err(|e| format!("Cannot remove {KEYS_FILE}: {e}"))?;
        removed += file.len();
    }
    Ok(removed)
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

/// A secret saved with `store_secret`. One an earlier version left in the credential store or
/// the keys file is copied into the vault as it is read, and stays where it was.
pub(crate) fn read_secret(name: &str) -> Option<String> {
    if let Some(value) = keys().get(name) {
        return Some(value.clone());
    }
    let status = vault::status();
    // The old stores' copies are not read past a password.
    if status == vault::Status::Waiting {
        return None;
    }
    let open = matches!(status, vault::Status::Open(_));
    let value = match vault::get(name) {
        // An empty secret stands for one a debug build removed.
        Some(value) => Some(value).filter(|v| !v.is_empty())?,
        None if status == vault::Status::Closed || imports(name) => {
            let value = legacy(name)?;
            if open && let Err(e) = vault::put(name, &value) {
                log::warn!(error:% = e; "could not move a secret into the vault: {{error}}");
            }
            value
        }
        None => return None,
    };
    keys().insert(name.into(), value.clone());
    Some(value)
}

/// The keys in the file, which is empty when there is no file.
///
/// # Errors
///
/// Returns an error when the file cannot be read or is not the JSON Patchwork wrote.
fn file_keys() -> Result<HashMap<String, String>, Error> {
    let path = dir().join(KEYS_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => return Err(format!("Cannot read {}: {e}", path.display()).into()),
    };
    if text.trim().is_empty() {
        return Ok(HashMap::new());
    }
    serde_json::from_str(&text)
        .map_err(|e| format!("{} is not valid JSON: {e}", path.display()).into())
}

fn read_file_keys() -> HashMap<String, String> {
    file_keys().unwrap_or_else(|e| {
        log::warn!(error:% = e; "could not read the stored API keys: {{error}}");
        HashMap::new()
    })
}

fn set_file_key(id: &str, key: Option<&str>) -> Result<(), Error> {
    let path = dir().join(KEYS_FILE);
    // A file that cannot be read is left alone: writing it would drop every other key in it.
    let mut file_keys = file_keys()?;
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
    provider.key_store?;
    read_secret(&provider.id)
}
