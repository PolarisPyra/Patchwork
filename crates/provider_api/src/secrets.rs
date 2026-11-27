//! Where provider secrets are kept: the vault, and behind it the system credential store,
//! where earlier versions kept them.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use patchwork_platform::{Error, Locked, keychain};
use patchwork_vault as vault;

use super::catalog::{ApiProvider, KeyStore};

/// The credential store's service name for every key Patchwork saves.
const SERVICE: &str = "dev.patchwork.ade";

/// Keys read this run, so a turn does not ask the credential store each time.
static KEYS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

fn keys() -> std::sync::MutexGuard<'static, HashMap<String, String>> {
    KEYS.get_or_init(Default::default).locked()
}

/// A secret's name in the credential store. A provider's key is named by the provider's id,
/// and its header values by `headers_secret`.
fn entry(name: &str) -> String {
    format!("api-provider:{name}")
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

/// The secret in the credential store, where an earlier version kept it.
fn legacy(name: &str) -> Option<String> {
    keychain::get(SERVICE, &entry(name)).ok().flatten()
}

/// Removes the secret from the credential store.
fn delete_legacy(name: &str) {
    let _ = keychain::delete(SERVICE, &entry(name));
}

/// Saves a secret in the vault, saying what keeps the vault's key.
///
/// A process that is not the app has no vault, and saves in the credential store.
///
/// # Errors
///
/// Returns an error when the vault is locked or cannot be written, or when a process that is
/// not the app finds no credential store: a secret is never written to a file as it is.
pub fn store_secret(name: &str, value: &str) -> Result<KeyStore, Error> {
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
        vault::Status::Locked(_) | vault::Status::Waiting | vault::Status::Unset => {
            // Fails with why the vault is locked.
            vault::put(name, value)?;
        }
        vault::Status::Closed => {}
    }
    keychain::set(SERVICE, &entry(name), value)
        .map_err(|e| format!("This system has no keychain to save the secret in: {e}"))?;
    keys().insert(name.to_string(), value.to_string());
    Ok(KeyStore::Keychain)
}

/// Removes a secret from the in-memory cache, vault, and legacy credential store.
pub fn delete_secret(name: &str) {
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
pub fn forget_secrets() {
    keys().clear();
}

/// How many of `names` the credential store still has a copy of.
#[must_use]
pub fn old_copies(names: &[String]) -> usize {
    names
        .iter()
        .filter(|name| matches!(keychain::get(SERVICE, &entry(name)), Ok(Some(_))))
        .count()
}

/// Removes the credential store's copies of `names`, which the vault holds.
///
/// Returns how many copies were removed.
///
/// # Errors
///
/// Returns an error when the vault is not open, or the credential store keeps a copy.
pub fn remove_old_copies(names: &[String]) -> Result<usize, Error> {
    if !matches!(vault::status(), vault::Status::Open(_)) {
        return Err("Patchwork's vault is not open".into());
    }
    let mut removed = 0;
    for name in names {
        if let Ok(Some(_)) = keychain::get(SERVICE, &entry(name)) {
            keychain::delete(SERVICE, &entry(name))
                .map_err(|e| format!("The system keychain kept an old copy: {e}"))?;
            removed += 1;
        }
    }
    Ok(removed)
}

/// The provider with its header values filled in from where they are stored.
pub(super) fn with_headers(mut provider: ApiProvider) -> ApiProvider {
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

/// A secret saved with `store_secret`.
///
/// One an earlier version left in the credential store is copied into the vault as it is read, and
/// stays where it was.
#[must_use]
pub fn read_secret(name: &str) -> Option<String> {
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

/// The provider's key: from its environment variable, else where Patchwork stored it.
#[must_use]
pub fn key(provider: &ApiProvider) -> Option<String> {
    if let Some(var) = &provider.key_env {
        return std::env::var(var)
            .ok()
            .filter(|v| !v.is_empty())
            .or_else(|| patchwork_platform::login_var(var));
    }
    provider.key_store?;
    read_secret(&provider.id)
}
