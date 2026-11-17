//! The vault's password: setting, changing and removing it, and locking and opening the
//! vault with it.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::sync::Arc;

use chacha20poly1305::aead::OsRng;
use chacha20poly1305::aead::rand_core::RngCore;
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use zeroize::Zeroizing;

use super::crypt::{DATA_KEY, KEY_LEN, SALT_LEN, cipher, data_key, derive, err, new_key, seal, unseal};
use super::state::{
    KEY_FILE, Method, Sealed, State, Status, Vault, new_wrapping_key, set_status, state, status,
};
use crate::{Core, Error, provider, settings};

/// The fewest characters a password has.
const MIN_PASSWORD: usize = 8;

impl Core {
    /// Opens the vault with its password, then starts what waited for its secrets: the
    /// terminal variables, the MCP servers and the chat apps. This blocks while the key is
    /// derived and the chat apps connect.
    ///
    /// # Errors
    ///
    /// Returns an error when the password is not the vault's, or the settings cannot be saved.
    pub fn unlock_vault(self: &Arc<Self>, password: &str) -> Result<(), Error> {
        unlock_with(password)?;
        settings::unseal(&self.store)?;
        provider::mcp::restart_changed();
        self.restart_gateway();
        Ok(())
    }

    /// Shuts a vault that has a password, and takes its secrets from what runs: the terminal
    /// variables, the MCP servers and the chat apps. It opens when the password is entered.
    ///
    /// # Errors
    ///
    /// Returns an error when the vault has no password, or the settings cannot be saved.
    pub fn lock_vault(self: &Arc<Self>) -> Result<(), Error> {
        lock()?;
        provider::api::config::forget_secrets();
        settings::forget(&self.store)?;
        provider::mcp::restart_changed();
        self.restart_gateway();
        Ok(())
    }

    /// Sets the vault's password, or changes it from `current`. From then on the vault opens
    /// only once the password is entered. This blocks while the key is derived.
    ///
    /// # Errors
    ///
    /// Returns an error when the vault is not open, `current` is not its password, `new` is
    /// too short, or the database cannot be written.
    pub fn set_vault_password(&self, current: &str, new: &str) -> Result<(), Error> {
        set_password(current, new)
    }

    /// Removes the vault's password: the system keychain keeps its key, or a key file.
    ///
    /// # Errors
    ///
    /// Returns an error when the vault is not open, `current` is not its password, or the
    /// database cannot be written.
    pub fn remove_vault_password(&self, current: &str) -> Result<(), Error> {
        remove_password(current)
    }
}

/// The data key, when `password` is the vault's.
fn data_key_with(password: &str) -> Result<Zeroizing<[u8; KEY_LEN]>, Error> {
    let row: Option<(Vec<u8>, Vec<u8>, Vec<u8>)> = {
        let state = state();
        let conn = match &*state {
            State::Waiting(sealed) => &sealed.conn,
            State::Open(vault) => &vault.conn,
            State::Closed | State::Locked(_) => return Err("Patchwork's vault is not open".into()),
        };
        conn.query_row(
            "SELECT nonce, wrapped_key, salt FROM vault WHERE id = 1 AND method = 'password'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(err)?
    };
    let (nonce, wrapped, salt) = row.ok_or("The vault has no password")?;
    // Derived without holding the vault, which other threads read meanwhile.
    let wrapping = derive(password, &salt)?;
    data_key(&wrapping, &nonce, &wrapped)
        .map_err(|_wrong_password| "That is not the vault's password".into())
}

/// Opens a vault that waits for its password.
fn unlock_with(password: &str) -> Result<(), Error> {
    let key = data_key_with(password)?;
    let mut state = state();
    match std::mem::replace(&mut *state, State::Closed) {
        State::Waiting(Sealed { conn, config }) => {
            *state = State::Open(Vault {
                conn,
                cipher: cipher(&key),
                method: Method::Password,
                config,
            });
            set_status(Status::Open(Method::Password));
            log::info!("the vault is open, by its password");
        }
        // Opened while the key was derived.
        other => *state = other,
    }
    Ok(())
}

/// Shuts the open vault, when a password opens it.
fn lock() -> Result<(), Error> {
    let mut state = state();
    match std::mem::replace(&mut *state, State::Closed) {
        State::Open(Vault {
            conn,
            method: Method::Password,
            config,
            ..
        }) => {
            *state = State::Waiting(Sealed { conn, config });
            set_status(Status::Waiting);
            log::info!("the vault is locked until its password is entered");
            Ok(())
        }
        other => {
            *state = other;
            Err("Only a vault with a password can be locked".into())
        }
    }
}

/// Seals every secret under a new data key, which `wrapping` seals, so that no key from
/// before opens the vault. All of it is saved, or none.
fn rekey(method: Method, wrapping: &[u8; KEY_LEN], salt: Option<&[u8]>) -> Result<(), Error> {
    let mut state = state();
    let State::Open(vault) = &mut *state else {
        return Err("Patchwork's vault is not open".into());
    };
    let key = new_key();
    let next = cipher(&key);
    let tx = vault
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(err)?;
    let rows: Vec<(String, Vec<u8>, Vec<u8>)> = tx
        .prepare("SELECT name, nonce, value FROM secrets")
        .and_then(|mut all| {
            all.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect()
        })
        .map_err(err)?;
    for (name, nonce, sealed) in rows {
        let value = unseal(&vault.cipher, &name, &nonce, &sealed)?;
        let (nonce, sealed) = seal(&next, &name, &value)?;
        tx.execute(
            "UPDATE secrets SET nonce = ?2, value = ?3 WHERE name = ?1",
            params![name, nonce, sealed],
        )
        .map_err(err)?;
    }
    let (nonce, wrapped) = seal(&cipher(wrapping), DATA_KEY, key.as_ref())?;
    tx.execute(
        "UPDATE vault SET method = ?1, nonce = ?2, wrapped_key = ?3, salt = ?4 WHERE id = 1",
        params![method.name(), nonce, wrapped, salt],
    )
    .map_err(err)?;
    tx.commit().map_err(err)?;
    vault.cipher = next;
    vault.method = method;
    set_status(Status::Open(method));
    log::info!(method = method.name(); "the vault has a new key, kept by {{method}}");
    Ok(())
}

/// The config folder of the open vault.
fn config() -> Result<PathBuf, Error> {
    match &*state() {
        State::Open(vault) => Ok(vault.config.clone()),
        _ => Err("Patchwork's vault is not open".into()),
    }
}

/// Sets the password of the open vault, or changes it from `current`.
fn set_password(current: &str, new: &str) -> Result<(), Error> {
    if new.chars().count() < MIN_PASSWORD {
        return Err(format!("A password has at least {MIN_PASSWORD} characters").into());
    }
    let config = config()?;
    let before = status();
    if before == Status::Open(Method::Password) {
        data_key_with(current)?;
    }
    let mut salt = [0; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    let wrapping = derive(new, &salt)?;
    rekey(Method::Password, &wrapping, Some(&salt))?;
    if before == Status::Open(Method::File) {
        // The key file opens nothing now.
        if let Err(e) = std::fs::remove_file(config.join(KEY_FILE)) {
            log::warn!(error:% = e; "could not remove the vault's old key file: {{error}}");
        }
    }
    Ok(())
}

/// Removes the password of the open vault, whose key the keychain or a key file then keeps.
fn remove_password(current: &str) -> Result<(), Error> {
    let config = config()?;
    data_key_with(current)?;
    let (method, wrapping) = new_wrapping_key(&config)?;
    rekey(method, &wrapping, None)
}
