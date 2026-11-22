//! How long a vault with a password stays open: the setting, and the key that keeps it open
//! across restarts when it never locks.
//!
//! The key kept is the wrapping key the password gives, in the system credential store. It
//! opens only the vault it was taken from: a new password seals the vault under a new one, and
//! the old one is then dropped as it fails.

// Rust guideline compliant 2026-10-09

use crate::system::{Locked, keychain};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::crypt::{KEY_LEN, decode_key};
use super::state::SERVICE;

/// When an open vault with a password locks by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VaultTimeout {
    /// After this many minutes without a key or the mouse in Patchwork's window.
    Minutes(u32),
    /// When the screen is locked or the computer goes to sleep.
    SystemLock,
    /// When Patchwork quits.
    Restart,
    /// Never: the system keychain keeps its key, so it opens as Patchwork starts.
    Never,
}

/// How a vault with a password is kept shut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", from = "Stored")]
pub struct VaultSettings {
    pub timeout: VaultTimeout,
}

impl Default for VaultSettings {
    fn default() -> Self {
        Self {
            timeout: VaultTimeout::SystemLock,
        }
    }
}

/// The settings as any version saved them: an earlier one kept the idle minutes and whether
/// the screen locks the vault.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Stored {
    timeout: Option<VaultTimeout>,
    lock_after_mins: u32,
    lock_with_screen: Option<bool>,
}

impl From<Stored> for VaultSettings {
    fn from(stored: Stored) -> Self {
        let timeout = stored.timeout.unwrap_or(match stored.lock_after_mins {
            0 if stored.lock_with_screen.unwrap_or(true) => VaultTimeout::SystemLock,
            0 => VaultTimeout::Restart,
            mins => VaultTimeout::Minutes(mins),
        });
        Self { timeout }
    }
}

/// The credential store's name for the key kept while the vault never locks. A debug build
/// has its own.
const ENTRY: &str = if cfg!(debug_assertions) {
    "vault-open:patchwork-dev"
} else {
    "vault-open:patchwork"
};

/// Whether the vault never locks, so the key it opens with is kept.
static KEEP: AtomicBool = AtomicBool::new(false);

/// The wrapping key the password gave this run, while the vault is open with it.
static HELD: Mutex<Option<Zeroizing<[u8; KEY_LEN]>>> = Mutex::new(None);

/// Says whether the vault never locks, as Patchwork starts and before the vault opens.
pub(super) fn start(keep: bool) {
    KEEP.store(keep, Ordering::Relaxed);
}

/// Says whether the vault never locks, and keeps the key it opened with or forgets it.
/// This waits on the system keychain.
pub(crate) fn keep_open(keep: bool) {
    KEEP.store(keep, Ordering::Relaxed);
    let held = HELD.locked().clone();
    match held {
        Some(wrapping) if keep => save(&wrapping),
        _ => forget(),
    }
}

/// Holds `wrapping`, which opened the vault, and keeps it in the keychain when the vault never
/// locks. This waits on the system keychain.
pub(super) fn hold(wrapping: &[u8; KEY_LEN]) {
    *HELD.locked() = Some(Zeroizing::new(*wrapping));
    if KEEP.load(Ordering::Relaxed) {
        save(wrapping);
    }
}

/// Lets go of the key, here and in the keychain, as the vault locks or stops having a password.
pub(super) fn release() {
    *HELD.locked() = None;
    forget();
}

/// The key kept for a vault that never locks, when there is one.
pub(super) fn kept() -> Option<Zeroizing<[u8; KEY_LEN]>> {
    if !KEEP.load(Ordering::Relaxed) {
        return None;
    }
    let text = Zeroizing::new(keychain::get(SERVICE, ENTRY).ok()??);
    decode_key(&text).ok()
}

fn save(wrapping: &[u8; KEY_LEN]) {
    let text = Zeroizing::new(STANDARD.encode(wrapping));
    if let Err(e) = keychain::set(SERVICE, ENTRY, &text) {
        log::warn!(error:% = e; "the vault will ask for its password next time: {{error}}");
    }
}

fn forget() {
    if let Err(e) = keychain::delete(SERVICE, ENTRY) {
        log::warn!(error:% = e; "could not drop the vault's kept key: {{error}}");
    }
}
