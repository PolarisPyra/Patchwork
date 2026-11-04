//! Patchwork's plans, what each one unlocks, and the license keys that prove a plan was bought.
//!
//! A license key is `<payload>.<signature>`, both unpadded base64url. The payload is the JSON of
//! [`License`]; the signature is its Ed25519 signature by the key whose public half is
//! [`PUBLIC_KEY`]. Keys are checked on the machine, so no server is asked. `docs/licensing.md`
//! says how to issue one.

use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The public half of the key that signs license keys.
const PUBLIC_KEY: [u8; 32] = [
    39, 220, 39, 67, 91, 19, 78, 214, 182, 27, 137, 173, 16, 12, 6, 44, 47, 164, 43, 226, 221, 43,
    239, 65, 250, 19, 9, 113, 108, 217, 35, 211,
];

/// What someone paid for. Without a license key, or with one that no longer holds, it is Free.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "license.ts")]
#[serde(rename_all = "lowercase")]
pub enum Plan {
    #[default]
    Free,
    Basic,
}

/// A paid feature, which a plan either includes or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "license.ts")]
#[serde(rename_all = "camelCase")]
pub enum Feature {
    /// A chat shows a background of its own instead of the one every chat shares.
    ChatBackgrounds,
}

impl Plan {
    /// The paid features this plan includes.
    pub fn features(self) -> &'static [Feature] {
        match self {
            Plan::Free => &[],
            Plan::Basic => &[Feature::ChatBackgrounds],
        }
    }

    pub fn allows(self, feature: Feature) -> bool {
        self.features().contains(&feature)
    }
}

/// What a license key says, once its signature is checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct License {
    /// The order the key was issued for, so a refunded one can be named.
    pub id: String,
    pub plan: Plan,
    /// Who bought it, shown in Settings → License.
    pub email: String,
    /// When the subscription runs out, in seconds since 1970.
    pub expires: u64,
}

impl License {
    /// The plan in effect at `now` (seconds since 1970): Free once the key has run out.
    pub fn plan_at(&self, now: u64) -> Plan {
        if now < self.expires {
            self.plan
        } else {
            Plan::Free
        }
    }
}

/// Why a license key was not taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyError {
    /// Not a license key: wrong shape, bad base64, or JSON that is not a license.
    Malformed,
    /// A license key, but not one Patchwork signed.
    Forged,
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            KeyError::Malformed => "That is not a Patchwork license key",
            KeyError::Forged => "That license key was not issued by Patchwork",
        })
    }
}

impl std::error::Error for KeyError {}

/// Reads a license key, checking that Patchwork signed it. An expired key still reads, so
/// Settings can say when it ran out; [`License::plan_at`] decides what it unlocks.
pub fn read_key(key: &str) -> Result<License, KeyError> {
    let (payload, signature) = key.trim().split_once('.').ok_or(KeyError::Malformed)?;
    let payload = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| KeyError::Malformed)?;
    let signature = URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| KeyError::Malformed)?;
    let signature = Signature::from_slice(&signature).map_err(|_| KeyError::Malformed)?;
    let verifier = VerifyingKey::from_bytes(&PUBLIC_KEY).map_err(|_| KeyError::Forged)?;
    verifier
        .verify_strict(&payload, &signature)
        .map_err(|_| KeyError::Forged)?;
    serde_json::from_slice(&payload).map_err(|_| KeyError::Malformed)
}
