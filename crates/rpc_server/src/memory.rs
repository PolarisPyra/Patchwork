//! In-memory [`PairingStore`] implementation for standalone tests and embedded harnesses.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::sync::Mutex;

use patchwork_platform::{Error, Locked};

use crate::{CodeGrant, PairedDevice, PairingStore};

/// How long a device's last-seen time may lag behind before updating, matching the SQLite store.
const SEEN_EVERY_MS: i64 = 60_000;

#[derive(Debug, Default)]
struct State {
    devices: Vec<(PairedDevice, String)>,
    pair_codes: HashMap<String, (Vec<String>, Option<String>, i64)>,
}

/// Thread-safe in-memory [`PairingStore`] backed by a mutex.
#[derive(Debug, Default)]
pub struct MemoryPairingStore {
    state: Mutex<State>,
}

impl MemoryPairingStore {
    /// Creates an empty in-memory pairing store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl PairingStore for MemoryPairingStore {
    fn paired_devices(&self) -> Result<Vec<PairedDevice>, Error> {
        let mut list: Vec<PairedDevice> = self
            .state
            .locked()
            .devices
            .iter()
            .map(|(paired, _)| paired.clone())
            .collect();
        list.sort_by(|a, b| {
            b.device
                .created_at_ms
                .cmp(&a.device.created_at_ms)
                .then_with(|| a.device.id.cmp(&b.device.id))
        });
        Ok(list)
    }

    fn device_by_token_hash(&self, token_hash: &str) -> Result<Option<PairedDevice>, Error> {
        Ok(self
            .state
            .locked()
            .devices
            .iter()
            .find(|(_, hash)| hash == token_hash)
            .map(|(paired, _)| paired.clone()))
    }

    fn paired_device(&self, id: &str) -> Result<Option<PairedDevice>, Error> {
        Ok(self
            .state
            .locked()
            .devices
            .iter()
            .find(|(paired, _)| paired.device.id == id)
            .map(|(paired, _)| paired.clone()))
    }

    fn add_device(&self, paired: &PairedDevice, token_hash: &str) -> Result<(), Error> {
        self.state
            .locked()
            .devices
            .push((paired.clone(), token_hash.to_string()));
        Ok(())
    }

    fn remove_device(&self, id: &str) -> Result<bool, Error> {
        let mut state = self.state.locked();
        let before = state.devices.len();
        state.devices.retain(|(paired, _)| paired.device.id != id);
        Ok(state.devices.len() < before)
    }

    fn update_device(
        &self,
        id: &str,
        name: Option<&str>,
        scopes: Option<&[String]>,
    ) -> Result<Option<PairedDevice>, Error> {
        let mut state = self.state.locked();
        let Some((paired, _)) = state
            .devices
            .iter_mut()
            .find(|(paired, _)| paired.device.id == id)
        else {
            return Ok(None);
        };
        if let Some(name) = name {
            paired.device.name = name.to_string();
        }
        if let Some(scopes) = scopes {
            paired.device.scopes = scopes.to_vec();
        }
        Ok(Some(paired.clone()))
    }

    fn touch_device(&self, id: &str, now_ms: i64) -> Result<(), Error> {
        let mut state = self.state.locked();
        if let Some((paired, _)) = state
            .devices
            .iter_mut()
            .find(|(paired, _)| paired.device.id == id)
            && paired
                .device
                .last_seen_ms
                .is_none_or(|seen| seen < now_ms - SEEN_EVERY_MS)
        {
            paired.device.last_seen_ms = Some(now_ms);
        }
        Ok(())
    }

    fn add_pair_code(
        &self,
        code: &str,
        scopes: &[String],
        name: Option<&str>,
        expires_at_ms: i64,
        now_ms: i64,
    ) -> Result<(), Error> {
        let mut state = self.state.locked();
        state
            .pair_codes
            .retain(|_, (_, _, expires)| *expires > now_ms);
        state.pair_codes.insert(
            code.to_string(),
            (scopes.to_vec(), name.map(str::to_string), expires_at_ms),
        );
        Ok(())
    }

    fn take_pair_code(&self, code: &str, now_ms: i64) -> Result<Option<CodeGrant>, Error> {
        let found = self.state.locked().pair_codes.remove(code);
        Ok(found
            .filter(|(_, _, expires_at)| *expires_at > now_ms)
            .map(|(scopes, name, _)| (scopes, name)))
    }
}
