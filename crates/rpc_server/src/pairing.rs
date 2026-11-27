//! Pairing a device: the short code a person types or scans, trading it for a token of the
//! device's own, and telling a token apart from the rest.

// Rust guideline compliant 2026-10-09

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine as _;
use chacha20poly1305::aead::OsRng;
use chacha20poly1305::aead::rand_core::RngCore;
use patchwork_platform::{Error, Locked};
use sha2::{Digest, Sha256};

use crate::{Device, PairedDevice, PairingStore, scopes};

/// How long a pairing code lasts: long enough to walk to a phone and type it, short enough that
/// a code seen over a shoulder is soon useless.
pub const CODE_LIFETIME_MS: i64 = 5 * 60_000;

/// What a code is made of: no vowels, so it cannot spell a word, and no 0, 1, O or I, which read
/// alike.
const ALPHABET: &[u8] = b"23456789BCDFGHJKLMNPQRSTVWXZ";

/// How many random bytes a device token holds: 256 bits, which no one guesses.
const TOKEN_BYTES: usize = 32;

/// How many characters of a token are kept to tell devices apart. Six base64 characters give
/// away 36 of its 256 bits.
const HINT_CHARS: usize = 6;

/// How many wrong codes may be tried in `GUESS_WINDOW` before claims are refused until the
/// window passes. A code is one of 28^6 (about 480 million) for five minutes, so at this rate
/// guessing one is hopeless, while a person who mistypes a few times is not locked out.
const MAX_WRONG_CODES: usize = 10;

/// The window `MAX_WRONG_CODES` counts over.
const GUESS_WINDOW: Duration = Duration::from_secs(60);

/// The scopes a device gets when its code was made without any.
pub const DEFAULT_SCOPES: [&str; 2] = [scopes::READ, scopes::WRITE];

/// Fills `bytes` from the system's random source.
fn random(bytes: &mut [u8]) {
    OsRng.fill_bytes(bytes);
}

/// A new token: random bytes as unpadded base64url, safe in a URL's query.
pub(crate) fn new_token(bytes: usize) -> String {
    let mut raw = vec![0u8; bytes];
    random(&mut raw);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw)
}

/// A new pairing code, such as `K7M-QX2`.
pub(crate) fn new_code() -> String {
    // Bytes at or above the largest multiple of the alphabet's length are drawn again, so every
    // letter is as likely as the others.
    let limit = 256 - 256 % ALPHABET.len();
    let mut code = String::with_capacity(7);
    while code.len() < 7 {
        let mut byte = [0u8; 1];
        random(&mut byte);
        let b = usize::from(byte[0]);
        if b >= limit {
            continue;
        }
        if code.len() == 3 {
            code.push('-');
        }
        code.push(char::from(ALPHABET[b % ALPHABET.len()]));
    }
    code
}

/// What a code typed by a person should be compared as: no spaces, upper case, and the dash in
/// its place, so `k7mqx2` and `K7M-QX2` are the same code.
pub(crate) fn normalize_code(typed: &str) -> String {
    let letters: String = typed
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if letters.len() == 6 {
        format!("{}-{}", &letters[..3], &letters[3..])
    } else {
        letters
    }
}

/// A token's SHA-256, as lowercase hex: what the database keeps in its place.
pub(crate) fn hash(token: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for b in Sha256::digest(token.as_bytes()) {
        out.push(char::from(HEX[usize::from(b >> 4)]));
        out.push(char::from(HEX[usize::from(b & 0xf)]));
    }
    out
}

/// Whether `given` is `expected`, taking as long whichever byte differs. Both are hashed first,
/// so the comparison is over equal lengths and says nothing about the token's own.
pub(crate) fn same_token(given: &str, expected: &str) -> bool {
    let (a, b) = (
        Sha256::digest(given.as_bytes()),
        Sha256::digest(expected.as_bytes()),
    );
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// The wrong codes tried lately, to slow down anyone guessing.
#[derive(Debug, Default)]
pub(crate) struct Guesses {
    wrong: Mutex<VecDeque<Instant>>,
}

impl Guesses {
    /// Whether too many wrong codes were tried lately for another claim to be taken.
    fn too_many(&self, now: Instant) -> bool {
        let mut wrong = self.wrong.locked();
        while wrong
            .front()
            .is_some_and(|at| now.duration_since(*at) > GUESS_WINDOW)
        {
            wrong.pop_front();
        }
        wrong.len() >= MAX_WRONG_CODES
    }

    fn note_wrong(&self, now: Instant) {
        self.wrong.locked().push_back(now);
    }
}

/// A code a device can be paired with, as [`start`] made it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Made {
    pub code: String,
    pub expires_at_ms: i64,
    pub scopes: Vec<String>,
}

/// Makes and saves a pairing code for a device with `scopes` (the default ones when empty),
/// named `name` when the person gave one.
///
/// # Errors
///
/// Returns an error if any scope is unknown or if saving the code fails.
pub fn start(
    store: &dyn PairingStore,
    scopes: Vec<String>,
    name: Option<&str>,
    now_ms: i64,
) -> Result<Made, Error> {
    if !scopes::known(&scopes) {
        return Err("A device can only be given the scopes read, write, terminal and admin".into());
    }
    let scopes = if scopes.is_empty() {
        DEFAULT_SCOPES.iter().map(ToString::to_string).collect()
    } else {
        scopes
    };
    let code = new_code();
    let expires_at_ms = now_ms + CODE_LIFETIME_MS;
    let name = name.map(str::trim).filter(|n| !n.is_empty());
    store.add_pair_code(&code, &scopes, name, expires_at_ms, now_ms)?;
    log::info!(
        expires_at_ms = expires_at_ms;
        "made a pairing code that expires at {{expires_at_ms}}"
    );
    Ok(Made {
        code,
        expires_at_ms,
        scopes,
    })
}

/// Trades a pairing code for a new device and its token. The code works once.
pub(crate) fn claim(
    store: &dyn PairingStore,
    guesses: &Guesses,
    code: &str,
    name: &str,
    platform: &str,
    now_ms: i64,
) -> Result<(String, PairedDevice), Error> {
    let now = Instant::now();
    if guesses.too_many(now) {
        log::warn!("refused a pairing claim: too many wrong codes in the last minute");
        return Err("Too many wrong codes were tried. Wait a minute and try again.".into());
    }
    let Some((scopes, named)) = store.take_pair_code(&normalize_code(code), now_ms)? else {
        guesses.note_wrong(now);
        log::warn!("refused a pairing claim: the code is wrong or has expired");
        return Err("That code is wrong or has expired".into());
    };
    let token = new_token(TOKEN_BYTES);
    let name = named
        .or_else(|| Some(name.trim().to_string()).filter(|n| !n.is_empty()))
        .unwrap_or_else(|| "Device".into());
    let platform = Some(platform.trim().to_string())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| "unknown".into());
    let paired = PairedDevice {
        device: Device {
            id: patchwork_platform::new_id(),
            name,
            platform,
            scopes,
            created_at_ms: now_ms,
            last_seen_ms: None,
        },
        token_hint: token.chars().take(HINT_CHARS).collect(),
    };
    store.add_device(&paired, &hash(&token))?;
    log::info!(
        device:% = paired.device.id,
        platform:% = paired.device.platform;
        "paired device {{device}} ({{platform}})"
    );
    Ok((token, paired))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_three_and_three_letters_of_the_alphabet() {
        for _ in 0..50 {
            let code = new_code();
            assert_eq!(code.len(), 7);
            assert_eq!(&code[3..4], "-");
            assert!(
                code.bytes()
                    .filter(|b| *b != b'-')
                    .all(|b| ALPHABET.contains(&b)),
                "{code}"
            );
        }
    }

    #[test]
    fn a_typed_code_is_read_however_it_is_written() {
        assert_eq!(normalize_code(" k7m qx2 "), "K7M-QX2");
        assert_eq!(normalize_code("K7M-QX2"), "K7M-QX2");
    }

    #[test]
    fn tokens_are_compared_whole() {
        let token = new_token(TOKEN_BYTES);
        assert_eq!(token.len(), 43);
        assert!(same_token(&token, &token));
        assert!(!same_token(&token[..42], &token));
        assert!(!same_token("", &token));
    }
}
