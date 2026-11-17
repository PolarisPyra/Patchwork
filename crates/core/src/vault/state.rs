//! The vault's state for the run: whether it is open, what keeps its key, and opening it as
//! the app starts.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chacha20poly1305::XChaCha20Poly1305;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use zeroize::Zeroizing;

use super::crypt::{DATA_KEY, KEY_LEN, cipher, data_key, decode_key, err, new_key, seal};
use crate::Error;
use crate::persistence::DB_FILE;
use crate::system::fs::write_private;

/// The credential store's service name for everything Patchwork saves.
const SERVICE: &str = "dev.patchwork.ade";

/// The wrapping key's name in the credential store. A debug build has its own.
const KEY_ENTRY: &str = if cfg!(debug_assertions) {
    "vault-key:patchwork-dev"
} else {
    "vault-key:patchwork"
};

/// The wrapping key's file in the config folder, on a system without a credential store.
pub(super) const KEY_FILE: &str = "vault.key";

/// How long a read or write waits for another Patchwork to finish with the database.
const BUSY: Duration = Duration::from_secs(5);

/// What keeps the vault's wrapping key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// The system credential store.
    Keychain,
    /// A file in the config folder, which anything running as the user can read.
    File,
    /// Nothing: the key is derived from a password the user enters each run.
    Password,
}

impl Method {
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Keychain => "keychain",
            Self::File => "file",
            Self::Password => "password",
        }
    }

    fn named(name: &str) -> Option<Self> {
        [Self::Keychain, Self::File, Self::Password]
            .into_iter()
            .find(|m| m.name() == name)
    }
}

/// Whether this run can use the vault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// The vault was not opened: this process is not the app.
    Closed,
    /// The vault's key could not be had, with why. Nothing can be saved.
    Locked(String),
    /// The vault has a password, which was not entered yet. Nothing can be read or saved.
    Waiting,
    Open(Method),
}

pub(super) struct Vault {
    pub(super) conn: Connection,
    pub(super) cipher: XChaCha20Poly1305,
    pub(super) method: Method,
    pub(super) config: PathBuf,
}

/// A vault that waits for its password.
pub(super) struct Sealed {
    pub(super) conn: Connection,
    pub(super) config: PathBuf,
}

pub(super) enum State {
    Closed,
    Locked(String),
    Waiting(Sealed),
    Open(Vault),
}

static STATE: Mutex<State> = Mutex::new(State::Closed);

/// What `status` answers, apart from `STATE`, so asking never waits on a read or a write.
static STATUS: Mutex<Status> = Mutex::new(Status::Closed);

pub(super) fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(super) fn set_status(status: Status) {
    *STATUS.lock().unwrap_or_else(PoisonError::into_inner) = status;
}

fn key_entry() -> keyring::Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, KEY_ENTRY)
}

/// The wrapping key of a vault that exists, from where `method` keeps it.
fn wrapping_key(method: Method, config: &Path) -> Result<Zeroizing<[u8; KEY_LEN]>, Error> {
    let text = match method {
        Method::Keychain => key_entry()
            .and_then(|e| e.get_password())
            .map_err(|e| format!("The system keychain did not give the vault's key: {e}"))?,
        Method::File => {
            let path = config.join(KEY_FILE);
            std::fs::read_to_string(&path)
                .map_err(|e| format!("Cannot read {}: {e}", path.display()))?
        }
        Method::Password => return Err("The vault opens with its password".into()),
    };
    decode_key(&Zeroizing::new(text))
}

/// A wrapping key for a new vault, and what keeps it: the credential store's, which every
/// database of this build shares, or a new one in the config folder's file without a store.
pub(super) fn new_wrapping_key(config: &Path) -> Result<(Method, Zeroizing<[u8; KEY_LEN]>), Error> {
    let kept = key_entry().and_then(|entry| match entry.get_password() {
        Ok(text) => Ok(Zeroizing::new(text)),
        Err(keyring::Error::NoEntry) => {
            let text = Zeroizing::new(STANDARD.encode(new_key().as_ref()));
            entry.set_password(&text)?;
            // A store that takes a key and cannot give it back would lock the vault for good.
            entry.get_password().map(Zeroizing::new)
        }
        Err(e) => Err(e),
    });
    match kept {
        Ok(text) => {
            if let Ok(key) = decode_key(&text) {
                return Ok((Method::Keychain, key));
            }
            log::warn!("the vault's key in the system keychain is damaged; using a key file");
        }
        Err(e) => {
            log::warn!(error:% = e; "no system keychain for the vault ({{error}}); using a key file");
        }
    }
    let key = new_key();
    let text = Zeroizing::new(STANDARD.encode(key.as_ref()));
    write_private(&config.join(KEY_FILE), &text)?;
    Ok((Method::File, key))
}

/// Opens the vault in the database of `data`, making it on the first run. A vault with a
/// password waits for it.
fn unlock(data: &Path, config: &Path) -> Result<State, Error> {
    let mut conn = Connection::open(data.join(DB_FILE)).map_err(err)?;
    conn.busy_timeout(BUSY).map_err(err)?;
    // Holds the database, so two Patchworks starting together cannot each make a vault.
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(err)?;
    let row: Option<(String, Vec<u8>, Vec<u8>)> = tx
        .query_row(
            "SELECT method, nonce, wrapped_key FROM vault WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(err)?;
    let config = config.to_path_buf();
    let (method, key) = if let Some((method, nonce, wrapped)) = row {
        let method = Method::named(&method).ok_or("The vault was made by a newer Patchwork")?;
        if method == Method::Password {
            drop(tx);
            return Ok(State::Waiting(Sealed { conn, config }));
        }
        let wrapping = wrapping_key(method, &config)?;
        let key = data_key(&wrapping, &nonce, &wrapped)
            .map_err(|_wrong_key| "The vault's key does not open this database")?;
        (method, key)
    } else {
        let (method, wrapping) = new_wrapping_key(&config)?;
        let key = new_key();
        let (nonce, wrapped) = seal(&cipher(&wrapping), DATA_KEY, key.as_ref())?;
        tx.execute(
            "INSERT INTO vault (id, method, nonce, wrapped_key) VALUES (1, ?1, ?2, ?3)",
            params![method.name(), nonce, wrapped],
        )
        .map_err(err)?;
        (method, key)
    };
    tx.commit().map_err(err)?;
    let cipher = cipher(&key);
    Ok(State::Open(Vault {
        conn,
        cipher,
        method,
        config,
    }))
}

/// Opens this instance's vault for the run. A vault that cannot be unlocked stays locked: its
/// secrets are left as they are, and nothing can be saved.
pub(crate) fn open(data: &Path, config: &Path) {
    let opened = unlock(data, config).unwrap_or_else(|e| State::Locked(e.to_string()));
    set_status(match &opened {
        State::Open(vault) => {
            log::info!(method = vault.method.name(); "the vault is open, its key kept by {{method}}");
            Status::Open(vault.method)
        }
        State::Waiting(_) => {
            log::info!("the vault waits for its password");
            Status::Waiting
        }
        State::Locked(why) => {
            log::warn!(error:% = why; "the vault is locked for this run: {{error}}");
            Status::Locked(why.clone())
        }
        State::Closed => Status::Closed,
    });
    *state() = opened;
}

/// Whether this run can use the vault, and what keeps its key.
#[must_use]
pub fn status() -> Status {
    STATUS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}
