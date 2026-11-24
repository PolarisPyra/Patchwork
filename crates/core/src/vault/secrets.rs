//! The secrets in the vault: reading, saving and removing one, listing them for Settings, and
//! clearing the copies earlier versions left outside the vault.

// Rust guideline compliant 2026-10-09

use rusqlite::{OptionalExtension, params};

use super::crypt::{err, seal, sealed_rows, unseal};
use super::known;
use super::password::needs_password;
use super::state::{State, Status, state, status};
use crate::gateway::Platform;
use crate::{Core, Error, provider};

/// A saved secret, for Settings to list.
#[derive(Debug, Clone)]
pub struct Saved {
    /// What the vault calls it.
    pub name: String,
    /// What it is, in words.
    pub label: String,
}

impl Core {
    /// The secrets saved in the vault, by what they are. None of their values.
    #[must_use]
    pub fn vault_secrets(&self) -> Vec<Saved> {
        let mut saved: Vec<Saved> = names()
            .into_iter()
            .map(|name| Saved {
                label: label(&name),
                name,
            })
            .collect();
        saved.sort_by_key(|s| s.label.to_lowercase());
        saved
    }

    /// The value of the saved secret `name`, for Settings to show when asked. No tool an agent
    /// or a chat app has reaches this. None while the vault's password has to be entered
    /// first (see `secret_needs_password`).
    #[must_use]
    pub fn reveal_secret(&self, name: &str) -> Option<String> {
        if needs_password() {
            return None;
        }
        get(name)
    }

    /// How many saved secrets the system keychain still holds a copy of, from an earlier
    /// version. A debug build shares the keychain with the released app, and counts none.
    #[must_use]
    pub fn old_secret_copies(&self) -> usize {
        if cfg!(debug_assertions) {
            return 0;
        }
        provider::api::old_copies(&names())
    }

    /// Removes those copies, and says how many.
    ///
    /// # Errors
    ///
    /// Returns an error in a debug build, when the vault is not open, or when a copy cannot be
    /// moved or removed.
    pub fn remove_old_secret_copies(&self) -> Result<usize, Error> {
        if cfg!(debug_assertions) {
            return Err("A debug build leaves the released app's stores alone".into());
        }
        provider::api::remove_old_copies(&names())
    }
}

/// What the secret `name` is, in words.
fn label(name: &str) -> String {
    if name == "terminal:env" {
        return "Terminal variables".into();
    }
    if let Some(server) = name.strip_prefix("mcp/") {
        return format!("MCP server {server}");
    }
    if let Some((id, key)) = name
        .strip_prefix("gateway:")
        .and_then(|rest| rest.split_once(':'))
        && let Some(platform) = Platform::ALL.into_iter().find(|p| p.id() == id)
    {
        let field = platform
            .credentials()
            .iter()
            .find(|f| f.key == key)
            .map_or(key, |f| f.label);
        return format!("{} {}", platform.label(), field.to_lowercase());
    }
    let (id, what) = name
        .strip_suffix(":headers")
        .map_or((name, "API key"), |id| (id, "headers"));
    // A secret whose provider is gone has only an id, which says nothing to the user.
    match provider::api::provider(id) {
        Some(provider) => format!("{} {what}", provider.name),
        None if what == "headers" => "API headers".into(),
        None => what.into(),
    }
}

/// Copies every secret an earlier version left in its stores into the vault, so the vault
/// holds them all, and not only the ones read so far. The keys file of an early version is
/// deleted once the vault has its keys; the credential store's copies stay where they were
/// until `remove_old_secret_copies`.
pub(crate) fn take_old() {
    if !matches!(status(), Status::Open(_)) {
        return;
    }
    let mut old: Vec<String> = Vec::new();
    for provider in provider::api::providers() {
        old.push(format!("{}:headers", provider.id));
        old.push(provider.id);
    }
    for platform in Platform::ALL {
        let id = platform.id();
        old.extend(
            platform
                .credentials()
                .iter()
                .map(|field| format!("gateway:{id}:{}", field.key)),
        );
    }
    match provider::api::take_keys_file() {
        Ok(0) => {}
        Ok(moved) => {
            log::info!(moved; "moved {{moved}} secrets from the old keys file into the vault");
        }
        Err(e) => log::warn!(error:% = e; "the old keys file was left in place: {{error}}"),
    }
    let before = names().len();
    for name in &old {
        let _ = provider::api::read_secret(name);
    }
    // Listing the MCP servers reads each one's saved values.
    let _ = provider::mcp::list();
    let taken = names().len().saturating_sub(before);
    if taken > 0 {
        log::info!(taken; "copied {{taken}} secrets from the old stores into the vault");
    }
}

/// The name of every saved secret. One saved empty stands for a removed one, and is left out.
pub(crate) fn names() -> Vec<String> {
    let state = state();
    let State::Open(vault) = &*state else {
        return Vec::new();
    };
    let rows = sealed_rows(&vault.conn).unwrap_or_else(|e| {
        log::warn!(error:% = e; "could not read the vault: {{error}}");
        Vec::new()
    });
    rows.into_iter()
        .filter(|(name, nonce, sealed)| {
            unseal(&vault.cipher, name, nonce, sealed).is_ok_and(|value| !value.is_empty())
        })
        .map(|(name, _, _)| name)
        .collect()
}

/// The secret saved as `name`. None when there is none, the vault is not open, or the secret
/// cannot be decrypted.
pub(crate) fn get(name: &str) -> Option<String> {
    let state = state();
    let State::Open(vault) = &*state else {
        return None;
    };
    let row: Option<(Vec<u8>, Vec<u8>)> = vault
        .conn
        .query_row(
            "SELECT nonce, value FROM secrets WHERE name = ?1",
            [name],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .unwrap_or_else(|e| {
            log::warn!(error:% = e; "could not read the vault: {{error}}");
            None
        });
    let (nonce, sealed) = row?;
    match unseal(&vault.cipher, name, &nonce, &sealed) {
        Ok(bytes) => String::from_utf8(bytes.to_vec()).ok(),
        Err(e) => {
            log::warn!(error:% = e; "a saved secret is unreadable: {{error}}");
            None
        }
    }
}

/// Saves `value` as the secret `name`, in place of what was there.
///
/// # Errors
///
/// Returns an error when the vault is not open, or the database cannot be written.
pub(crate) fn put(name: &str, value: &str) -> Result<(), Error> {
    let state = state();
    let vault = match &*state {
        State::Open(vault) => vault,
        State::Locked(why) => {
            return Err(format!("Patchwork cannot save secrets: {why}").into());
        }
        State::Waiting(_) => {
            return Err("Enter the vault's password in Settings, under Vault, first".into());
        }
        State::Unset(_) => {
            return Err(
                "Set a password in Settings, under Vault, first: this system has \
                        no keychain to lock saved secrets"
                    .into(),
            );
        }
        State::Closed => return Err("Patchwork's vault is not open".into()),
    };
    let (nonce, sealed) = seal(&vault.cipher, name, value.as_bytes())?;
    vault
        .conn
        .execute(
            "INSERT INTO secrets (name, nonce, value) VALUES (?1, ?2, ?3)
             ON CONFLICT (name) DO UPDATE SET nonce = excluded.nonce, value = excluded.value",
            params![name, nonce, sealed],
        )
        .map_err(err)?;
    known::refresh(vault);
    Ok(())
}

/// Removes the secret `name`. Nothing happens when the vault is not open.
///
/// # Errors
///
/// Returns an error when the database cannot be written.
pub(crate) fn remove(name: &str) -> Result<(), Error> {
    let state = state();
    let State::Open(vault) = &*state else {
        return Ok(());
    };
    vault
        .conn
        .execute("DELETE FROM secrets WHERE name = ?1", [name])
        .map_err(err)?;
    known::refresh(vault);
    Ok(())
}
