//! The vault's password: setting, changing and removing it, and locking and opening the
//! vault with it.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use chacha20poly1305::aead::OsRng;
use chacha20poly1305::aead::rand_core::RngCore;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::crypt::{DATA_KEY, KEY_LEN, SALT_LEN, cipher, data_key, derive, err, new_key, seal, unseal};
use super::known;
use super::recovery::new_recovery;
use super::state::{
    KEY_FILE, Method, Sealed, State, Status, Vault, new_wrapping_key, set_status, state, status,
};
use crate::{Core, Error, provider, settings};

/// How a vault with a password is kept shut.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VaultSettings {
    /// The minutes without a key or the mouse in Patchwork's window after which the vault
    /// locks. 0 never locks it.
    pub lock_after_mins: u32,
}

/// The fewest characters a password has.
const MIN_PASSWORD: usize = 8;

/// How long after the password was entered a saved secret is shown without asking for it.
const CONFIRM_FOR: Duration = Duration::from_secs(5 * 60);

/// When the vault's password was last entered this run.
static CONFIRMED: Mutex<Option<Instant>> = Mutex::new(None);

fn set_confirmed(at: Option<Instant>) {
    *CONFIRMED.lock().unwrap_or_else(PoisonError::into_inner) = at;
}

/// Whether showing a saved secret takes the password first: the vault has one, and it was
/// not entered in the last few minutes.
pub(super) fn needs_password() -> bool {
    if status() != Status::Open(Method::Password) {
        return false;
    }
    let confirmed = *CONFIRMED.lock().unwrap_or_else(PoisonError::into_inner);
    confirmed.is_none_or(|at| at.elapsed() >= CONFIRM_FOR)
}

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
        self.vault_opened()
    }

    /// Starts what waited for the vault's secrets.
    pub(super) fn vault_opened(self: &Arc<Self>) -> Result<(), Error> {
        settings::unseal(&self.store)?;
        super::secrets::take_old();
        provider::mcp::restart_changed();
        self.restart_gateway();
        Ok(())
    }

    /// Whether showing a saved secret takes the vault's password first.
    #[must_use]
    pub fn secret_needs_password(&self) -> bool {
        needs_password()
    }

    /// Takes the vault's password, so saved secrets are shown for the next few minutes. This
    /// blocks while the key is derived.
    ///
    /// # Errors
    ///
    /// Returns an error when the vault is not open, or the password is not the vault's.
    pub fn confirm_vault_password(&self, password: &str) -> Result<(), Error> {
        data_key_with(password)?;
        set_confirmed(Some(Instant::now()));
        Ok(())
    }

    /// Shuts a vault that has a password, and takes its secrets from the terminal variables,
    /// the MCP servers and the chat apps. It opens when the password is entered. The agents
    /// that run keep what they started with: `lock_vault` stops those too.
    ///
    /// # Errors
    ///
    /// Returns an error when the vault has no password, or the settings cannot be saved.
    pub(crate) fn shut_vault(self: &Arc<Self>) -> Result<(), Error> {
        lock()?;
        provider::api::config::forget_secrets();
        settings::forget(&self.store)?;
        provider::mcp::restart_changed();
        self.restart_gateway();
        Ok(())
    }

    /// Sets the vault's password, or changes it from `current`, and gives the new recovery
    /// key, which opens the vault when the password is lost. It is kept nowhere: the user
    /// writes it down now. From then on the vault opens only once the password is entered.
    /// This blocks while the key is derived.
    ///
    /// # Errors
    ///
    /// Returns an error when the vault is not open, `current` is not its password, `new` is
    /// too short, or the database cannot be written.
    pub fn set_vault_password(&self, current: &str, new: &str) -> Result<String, Error> {
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

/// Runs `read` on the vault's database, whether the vault is open or waits for its password.
pub(super) fn with_conn<T>(
    read: impl FnOnce(&Connection) -> rusqlite::Result<T>,
) -> Result<T, Error> {
    let state = state();
    let conn = match &*state {
        State::Waiting(sealed) => &sealed.conn,
        State::Open(vault) => &vault.conn,
        State::Closed | State::Locked(_) => return Err("Patchwork's vault is not open".into()),
    };
    read(conn).map_err(err)
}

/// The data key, when `password` is the vault's.
pub(super) fn data_key_with(password: &str) -> Result<Zeroizing<[u8; KEY_LEN]>, Error> {
    let row: Option<(Vec<u8>, Vec<u8>, Vec<u8>)> = with_conn(|conn| {
        conn.query_row(
            "SELECT nonce, wrapped_key, salt FROM vault WHERE id = 1 AND method = 'password'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
    })?;
    let (nonce, wrapped, salt) = row.ok_or("The vault has no password")?;
    // Derived without holding the vault, which other threads read meanwhile.
    let wrapping = derive(password, &salt)?;
    data_key(&wrapping, &nonce, &wrapped)
        .map_err(|_wrong_password| "That is not the vault's password".into())
}

/// Opens a vault that waits for its password.
fn unlock_with(password: &str) -> Result<(), Error> {
    let key = data_key_with(password)?;
    open_with(&key);
    Ok(())
}

/// Opens a vault that waits for its password with its data key, which the password or the
/// recovery key gave.
pub(super) fn open_with(key: &[u8; KEY_LEN]) {
    let mut state = state();
    match std::mem::replace(&mut *state, State::Closed) {
        State::Waiting(Sealed { conn, config }) => {
            let vault = Vault {
                conn,
                cipher: cipher(key),
                method: Method::Password,
                config,
            };
            known::refresh(&vault);
            *state = State::Open(vault);
            set_status(Status::Open(Method::Password));
            log::info!("the vault is open, by its password");
        }
        // Opened while the key was derived.
        other => *state = other,
    }
    drop(state);
    set_confirmed(Some(Instant::now()));
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
            known::clear();
            set_confirmed(None);
            log::info!("the vault is locked until its password is entered");
            Ok(())
        }
        other => {
            *state = other;
            Err("Only a vault with a password can be locked".into())
        }
    }
}

/// Seals every secret under a new data key, which `wrapping` seals, and `recovery` too when
/// there is one, so that no key from before opens the vault. All of it is saved, or none.
fn rekey(
    method: Method,
    wrapping: &[u8; KEY_LEN],
    salt: Option<&[u8]>,
    recovery: Option<&[u8; KEY_LEN]>,
) -> Result<(), Error> {
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
    let (recovery_nonce, recovery_key) = recovery
        .map(|recovery| seal(&cipher(recovery), DATA_KEY, key.as_ref()))
        .transpose()?
        .unzip();
    tx.execute(
        "UPDATE vault SET method = ?1, nonce = ?2, wrapped_key = ?3, salt = ?4,
         recovery_nonce = ?5, recovery_key = ?6 WHERE id = 1",
        params![
            method.name(),
            nonce,
            wrapped,
            salt,
            recovery_nonce,
            recovery_key
        ],
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

/// Whether `new` is long enough for a password.
pub(super) fn long_enough(new: &str) -> Result<(), Error> {
    if new.chars().count() < MIN_PASSWORD {
        return Err(format!("A password has at least {MIN_PASSWORD} characters").into());
    }
    Ok(())
}

/// Sets the password of the open vault, or changes it from `current`. Gives the new recovery
/// key.
fn set_password(current: &str, new: &str) -> Result<String, Error> {
    long_enough(new)?;
    if status() == Status::Open(Method::Password) {
        data_key_with(current)?;
    }
    change_password(new)
}

/// Makes `new` the password of the open vault, whatever kept its key, and gives the new
/// recovery key.
pub(super) fn change_password(new: &str) -> Result<String, Error> {
    let config = config()?;
    let before = status();
    let mut salt = [0; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    let wrapping = derive(new, &salt)?;
    let (shown, recovery) = new_recovery();
    rekey(Method::Password, &wrapping, Some(&salt), Some(&recovery))?;
    set_confirmed(Some(Instant::now()));
    if before == Status::Open(Method::File) {
        // The key file opens nothing now.
        if let Err(e) = std::fs::remove_file(config.join(KEY_FILE)) {
            log::warn!(error:% = e; "could not remove the vault's old key file: {{error}}");
        }
    }
    Ok(shown)
}

/// Removes the password of the open vault, whose key the keychain or a key file then keeps.
fn remove_password(current: &str) -> Result<(), Error> {
    let config = config()?;
    data_key_with(current)?;
    let (method, wrapping) = new_wrapping_key(&config)?;
    rekey(method, &wrapping, None, None)
}
