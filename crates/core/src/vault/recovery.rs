//! The vault's recovery key: a second way in for a vault with a password, shown once when
//! the password is set, for the day the password is lost.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use chacha20poly1305::aead::OsRng;
use chacha20poly1305::aead::rand_core::RngCore;
use rusqlite::{OptionalExtension, params};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use super::crypt::{DATA_KEY, KEY_LEN, cipher, data_key, seal};
use super::password::{change_password, data_key_with, long_enough, open_with, with_conn};
use crate::{Core, Error};

/// The letters of a recovery key: none that reads as another, so it can be copied by hand.
const LETTERS: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// How many letters a recovery key has. Each is five random bits, 160 in all.
const LENGTH: usize = 32;

/// What a recovery key is hashed with, so its wrapping key is of no use elsewhere.
const DOMAIN: &[u8] = b"patchwork vault recovery key\0";

impl Core {
    /// Opens a vault whose password is lost with its recovery key, makes `new` its password,
    /// and gives the recovery key that replaces the one used. Then starts what waited for the
    /// vault's secrets. This blocks while the key is derived and the chat apps connect.
    ///
    /// # Errors
    ///
    /// Returns an error when the vault has no recovery key, `recovery` is not it, `new` is
    /// too short, or the database cannot be written.
    pub fn recover_vault(self: &Arc<Self>, recovery: &str, new: &str) -> Result<String, Error> {
        let shown = recover(recovery, new)?;
        // The password is changed by now, and the new recovery key has to reach the user.
        if let Err(e) = self.vault_opened() {
            log::warn!(error:% = e; "the vault opened, and its settings were not read: {{error}}");
        }
        Ok(shown)
    }

    /// Replaces the vault's recovery key, or makes one for a vault whose password is older
    /// than recovery keys. The one before opens nothing. This blocks while the key is derived.
    ///
    /// # Errors
    ///
    /// Returns an error when `current` is not the vault's password, or the database cannot be
    /// written.
    pub fn new_recovery_key(&self, current: &str) -> Result<String, Error> {
        let key = data_key_with(current)?;
        let (shown, recovery) = new_recovery();
        let (nonce, wrapped) = seal(&cipher(&recovery), DATA_KEY, key.as_ref())?;
        with_conn(|conn| {
            conn.execute(
                "UPDATE vault SET recovery_nonce = ?1, recovery_key = ?2
                 WHERE id = 1 AND method = 'password'",
                params![nonce, wrapped],
            )
        })?;
        log::info!("the vault has a new recovery key");
        Ok(shown)
    }

    /// Whether the vault has a password and a recovery key for it.
    #[must_use]
    pub fn vault_has_recovery_key(&self) -> bool {
        sealed_key().is_ok_and(|row| row.is_some())
    }
}

/// A new recovery key: as the user reads it, in groups of four, and the key it wraps with.
pub(super) fn new_recovery() -> (String, Zeroizing<[u8; KEY_LEN]>) {
    let mut random = Zeroizing::new([0_u8; LENGTH]);
    OsRng.fill_bytes(random.as_mut());
    // Thirty-two letters: the low five bits of a byte pick one evenly.
    let letters: Zeroizing<Vec<u8>> = Zeroizing::new(
        random
            .iter()
            .map(|b| LETTERS[usize::from(b & 31)])
            .collect(),
    );
    let shown = letters
        .chunks(4)
        .map(|group| String::from_utf8_lossy(group))
        .collect::<Vec<_>>()
        .join("-");
    let wrapping = wrapping(&shown);
    (shown, wrapping)
}

/// The key a recovery key wraps the data key with, however the user typed it: in either
/// case, with or without its dashes and spaces.
fn wrapping(recovery: &str) -> Zeroizing<[u8; KEY_LEN]> {
    let typed: Zeroizing<String> = Zeroizing::new(
        recovery
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_uppercase())
            .collect(),
    );
    let mut hash = Sha256::new();
    hash.update(DOMAIN);
    hash.update(typed.as_bytes());
    let mut key = Zeroizing::new([0; KEY_LEN]);
    key.copy_from_slice(&hash.finalize());
    key
}

type Bytes = Vec<u8>;

/// A sealed data key: its nonce, and the key as it was sealed.
type Sealed = (Bytes, Bytes);

/// The data key as the recovery key sealed it: None for a vault without one.
fn sealed_key() -> Result<Option<Sealed>, Error> {
    let row: Option<(Option<Bytes>, Option<Bytes>)> = with_conn(|conn| {
        conn.query_row(
            "SELECT recovery_nonce, recovery_key FROM vault WHERE id = 1 AND method = 'password'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
    })?;
    Ok(row.and_then(|(nonce, wrapped)| nonce.zip(wrapped)))
}

/// Opens the vault with its recovery key and makes `new` its password.
fn recover(recovery: &str, new: &str) -> Result<String, Error> {
    long_enough(new)?;
    let (nonce, wrapped) = sealed_key()?.ok_or("The vault has no recovery key")?;
    let key = data_key(&wrapping(recovery), &nonce, &wrapped)
        .map_err(|_wrong_key| "That is not the vault's recovery key")?;
    open_with(&key);
    log::info!("the vault was opened with its recovery key");
    change_password(new)
}
