//! The vault's state for the run: whether it is open, what keeps its key, and opening it as
//! the app starts.

// Rust guideline compliant 2026-10-09

use crate::system::Locked;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chacha20poly1305::XChaCha20Poly1305;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use zeroize::Zeroizing;

use super::crypt::{DATA_KEY, KEY_LEN, cipher, data_key, decode_key, err, held, new_key, seal};
use super::known;
use crate::Error;
use crate::persistence::DB_FILE;

/// The credential store's service name for everything Patchwork saves.
pub(super) const SERVICE: &str = "dev.patchwork.ade";

/// The wrapping key's name in the credential store. A debug build has its own.
const KEY_ENTRY: &str = if cfg!(debug_assertions) {
    "vault-key:patchwork-dev"
} else {
    "vault-key:patchwork"
};

/// The wrapping key's file in the config folder, which an earlier version wrote on a system
/// without a credential store. It is read until the vault has a password, never written.
pub(super) const KEY_FILE: &str = "vault.key";

/// How long a read or write waits for another Patchwork to finish with the database.
const BUSY: Duration = Duration::from_secs(5);

/// What keeps the vault's wrapping key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// The system credential store.
    Keychain,
    /// A file in the config folder, which anything running as the user can read. An earlier
    /// version made these: no vault starts with one, and setting a password removes it.
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
    /// The vault has no key yet: the system has no credential store, and no password was
    /// set. Nothing can be saved until one is.
    Unset,
    Open(Method),
}

pub(super) struct Vault {
    pub(super) conn: Connection,
    pub(super) cipher: Box<XChaCha20Poly1305>,
    pub(super) method: Method,
    pub(super) config: PathBuf,
}

/// A vault that waits for its password, or for a password to be set.
pub(super) struct Sealed {
    pub(super) conn: Connection,
    pub(super) config: PathBuf,
}

pub(super) enum State {
    Closed,
    Locked(String),
    Waiting(Sealed),
    Unset(Sealed),
    Open(Vault),
}

static STATE: Mutex<State> = Mutex::new(State::Closed);

/// What `status` answers, apart from `STATE`, so asking never waits on a read or a write.
static STATUS: Mutex<Status> = Mutex::new(Status::Closed);

pub(super) fn state() -> MutexGuard<'static, State> {
    STATE.locked()
}

pub(super) fn set_status(status: Status) {
    *STATUS.locked() = status;
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

/// The credential store's wrapping key, which every database of this build shares, made on
/// its first use.
///
/// # Errors
///
/// Returns an error when the system has no credential store, or its key is damaged.
pub(super) fn keychain_key() -> Result<Zeroizing<[u8; KEY_LEN]>, Error> {
    let entry = key_entry().map_err(|e| no_keychain(&e))?;
    let text = match entry.get_password() {
        Ok(text) => Zeroizing::new(text),
        Err(keyring::Error::NoEntry) => {
            let text = Zeroizing::new(STANDARD.encode(new_key().as_ref()));
            entry.set_password(&text).map_err(|e| no_keychain(&e))?;
            // A store that takes a key and cannot give it back would lock the vault for good.
            entry
                .get_password()
                .map(Zeroizing::new)
                .map_err(|e| no_keychain(&e))?
        }
        Err(e) => return Err(no_keychain(&e)),
    };
    decode_key(&text).map_err(|_damaged| "The vault's key in the system keychain is damaged".into())
}

fn no_keychain(e: &keyring::Error) -> Error {
    format!("The system keychain cannot keep the vault's key: {e}").into()
}

/// Opens the vault in the database of `data`, making it on the first run. A vault with a
/// password waits for it, and a system without a credential store makes none until a password
/// is set.
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
            // A vault that never locks opens with the key kept for it, while that still fits.
            let kept = super::timeout::kept().and_then(|wrapping| {
                let key = data_key(&wrapping, &nonce, &wrapped).ok()?;
                super::timeout::hold(&wrapping);
                Some(key)
            });
            let Some(key) = kept else {
                return Ok(State::Waiting(Sealed { conn, config }));
            };
            return Ok(State::Open(Vault {
                conn,
                cipher: held(&key),
                method,
                config,
            }));
        }
        let wrapping = wrapping_key(method, &config)?;
        let key = data_key(&wrapping, &nonce, &wrapped)
            .map_err(|_wrong_key| "The vault's key does not open this database")?;
        (method, key)
    } else {
        let wrapping = match keychain_key() {
            Ok(wrapping) => wrapping,
            Err(e) => {
                drop(tx);
                log::warn!(error:% = e; "the vault needs a password: {{error}}");
                return Ok(State::Unset(Sealed { conn, config }));
            }
        };
        let method = Method::Keychain;
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
    let cipher = held(&key);
    Ok(State::Open(Vault {
        conn,
        cipher,
        method,
        config,
    }))
}

/// Opens this instance's vault for the run. A vault that cannot be unlocked stays locked: its
/// secrets are left as they are, and nothing can be saved. A vault with a password waits for
/// it, unless it never locks (`keep`) and its kept key still opens it.
pub(crate) fn open(data: &Path, config: &Path, keep: bool) {
    super::timeout::start(keep);
    let opened = unlock(data, config).unwrap_or_else(|e| State::Locked(e.to_string()));
    set_status(match &opened {
        State::Open(vault) => {
            known::refresh(vault);
            log::info!(method = vault.method.name(); "the vault is open, its key kept by {{method}}");
            Status::Open(vault.method)
        }
        State::Waiting(_) => {
            log::info!("the vault waits for its password");
            Status::Waiting
        }
        State::Unset(_) => Status::Unset,
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
    STATUS.locked().clone()
}
