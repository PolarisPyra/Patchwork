//! The secrets in the vault: reading, saving, removing, and listing secret names.

// Rust guideline compliant 2026-10-09

use patchwork_platform::Error;
use rusqlite::{OptionalExtension, params};

use super::crypt::{err, seal, sealed_rows, unseal};
use super::known;
use super::state::{State, state};

/// A saved secret, for Settings to list.
#[derive(Debug, Clone)]
pub struct Saved {
    /// What the vault calls it.
    pub name: String,
    /// What it is, in words.
    pub label: String,
}

/// The name of every saved secret. One saved empty stands for a removed one, and is left out.
#[must_use]
pub fn names() -> Vec<String> {
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
#[must_use]
pub fn get(name: &str) -> Option<String> {
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
pub fn put(name: &str, value: &str) -> Result<(), Error> {
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
pub fn remove(name: &str) -> Result<(), Error> {
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
