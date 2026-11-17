//! The vault's ciphers: sealing and opening a secret, and the keys that do it.

// Rust guideline compliant 2026-10-09

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chacha20poly1305::aead::rand_core::RngCore;
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use zeroize::Zeroizing;

use crate::Error;

/// What the data key is sealed as, in place of a secret's name.
pub(super) const DATA_KEY: &str = "patchwork vault data key";

/// The length of a key, in bytes.
pub(super) const KEY_LEN: usize = 32;

/// The length of the salt a password's key is derived with, in bytes.
pub(super) const SALT_LEN: usize = 16;

/// What Argon2id spends deriving a key from the password: memory in KiB, passes and lanes.
/// Changing one locks every vault that has a password.
const ARGON: (u32, u32, u32) = (64 * 1024, 3, 4);

pub(super) fn err(e: impl std::fmt::Display) -> Error {
    format!("Vault error: {e}").into()
}

/// A new random key.
pub(super) fn new_key() -> Zeroizing<[u8; KEY_LEN]> {
    let mut key = Zeroizing::new([0; KEY_LEN]);
    OsRng.fill_bytes(key.as_mut());
    key
}

pub(super) fn cipher(key: &[u8; KEY_LEN]) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new(Key::from_slice(key))
}

/// The cipher the open vault keeps, where the swap file never gets its key: in memory that
/// does not move and is not written out.
#[expect(
    clippy::unnecessary_box_returns,
    reason = "the box is what keeps the pinned key at one address"
)]
pub(super) fn held(key: &[u8; KEY_LEN]) -> Box<XChaCha20Poly1305> {
    let held = Box::new(cipher(key));
    if let Err(e) = crate::system::memory::pin(&*held) {
        log::warn!(error:% = e; "the vault's key could be written to the swap file: {{error}}");
    }
    held
}

/// `value` sealed as `name`: the nonce it was sealed with, and the sealed bytes.
pub(super) fn seal(
    cipher: &XChaCha20Poly1305,
    name: &str,
    value: &[u8],
) -> Result<(Vec<u8>, Vec<u8>), Error> {
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let sealed = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: value,
                aad: name.as_bytes(),
            },
        )
        .map_err(|e| format!("Patchwork could not encrypt a secret: {e}"))?;
    Ok((nonce.to_vec(), sealed))
}

/// What was sealed as `name`. An error when the bytes were changed, sealed under another key,
/// or sealed as another name.
pub(super) fn unseal(
    cipher: &XChaCha20Poly1305,
    name: &str,
    nonce: &[u8],
    sealed: &[u8],
) -> Result<Zeroizing<Vec<u8>>, Error> {
    if nonce.len() != XNonce::default().len() {
        return Err("A saved secret is damaged".into());
    }
    cipher
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: sealed,
                aad: name.as_bytes(),
            },
        )
        .map(Zeroizing::new)
        .map_err(|e| format!("A saved secret could not be decrypted: {e}").into())
}

/// A key from the text it is kept as.
pub(super) fn decode_key(text: &str) -> Result<Zeroizing<[u8; KEY_LEN]>, Error> {
    let bytes = Zeroizing::new(
        STANDARD
            .decode(text.trim())
            // The decoder's error would quote a byte of the key.
            .map_err(|_damaged| "The vault's key is damaged")?,
    );
    let mut key = Zeroizing::new([0; KEY_LEN]);
    if bytes.len() != KEY_LEN {
        return Err("The vault's key is damaged".into());
    }
    key.copy_from_slice(&bytes);
    Ok(key)
}

/// The data key that `wrapping` seals as `wrapped`.
pub(super) fn data_key(
    wrapping: &[u8; KEY_LEN],
    nonce: &[u8],
    wrapped: &[u8],
) -> Result<Zeroizing<[u8; KEY_LEN]>, Error> {
    let bytes = unseal(&cipher(wrapping), DATA_KEY, nonce, wrapped)?;
    let mut key = Zeroizing::new([0; KEY_LEN]);
    if bytes.len() != KEY_LEN {
        return Err("The vault's key is damaged".into());
    }
    key.copy_from_slice(&bytes);
    Ok(key)
}

/// The wrapping key `password` gives with `salt`.
pub(super) fn derive(password: &str, salt: &[u8]) -> Result<Zeroizing<[u8; KEY_LEN]>, Error> {
    let (memory, passes, lanes) = ARGON;
    let params = Params::new(memory, passes, lanes, Some(KEY_LEN)).map_err(err)?;
    let mut key = Zeroizing::new([0; KEY_LEN]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .map_err(err)?;
    Ok(key)
}
