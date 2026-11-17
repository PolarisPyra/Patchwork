//! The secrets in the vault: reading, saving and removing one, listing them for Settings, and
//! clearing the copies earlier versions left outside the vault.

// Rust guideline compliant 2026-10-09

use rusqlite::{OptionalExtension, params};

use super::crypt::{err, seal, unseal};
use super::state::{State, state};
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
    /// or a chat app has reaches this.
    #[must_use]
    pub fn reveal_secret(&self, name: &str) -> Option<String> {
        get(name)
    }

    /// How many saved secrets an earlier version's stores still hold a copy of: the system
    /// keychain's own entries and `api-keys.json`. A debug build shares those stores with the
    /// released app, and counts none.
    #[must_use]
    pub fn old_secret_copies(&self) -> usize {
        if cfg!(debug_assertions) {
            return 0;
        }
        provider::api::config::old_copies(&names())
    }

    /// Removes those copies, and says how many. A secret only `api-keys.json` holds is moved
    /// into the vault first.
    ///
    /// # Errors
    ///
    /// Returns an error in a debug build, when the vault is not open, or when a copy cannot be
    /// moved or removed.
    pub fn remove_old_secret_copies(&self) -> Result<usize, Error> {
        if cfg!(debug_assertions) {
            return Err("A debug build leaves the released app's stores alone".into());
        }
        provider::api::config::remove_old_copies(&names())
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
    let provider = provider::api::config::provider(id).map_or_else(|| id.to_string(), |p| p.name);
    format!("{provider} {what}")
}

/// The name of every saved secret. One saved empty stands for a removed one, and is left out.
pub(crate) fn names() -> Vec<String> {
    let state = state();
    let State::Open(vault) = &*state else {
        return Vec::new();
    };
    let rows: Vec<(String, Vec<u8>, Vec<u8>)> = vault
        .conn
        .prepare("SELECT name, nonce, value FROM secrets")
        .and_then(|mut all| {
            all.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect()
        })
        .unwrap_or_else(|e| {
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
            return Err("Enter the vault's password in Settings, under Security, first".into());
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
    Ok(())
}
