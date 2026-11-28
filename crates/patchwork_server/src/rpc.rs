//! Request caller authentication, scope checks, and server-owned RPC methods (pairing, devices,
//! and the event sequence log). Every other method is dispatched to the [`Handler`].

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use patchwork_protocol::devices::{Claimed, Device, EventsSince, PairingCode, PairingInfo};
use patchwork_platform::Error;
use serde::Serialize;
use serde_json::Value;

use super::listener::Live;
use super::{PairedDevice, now_ms, pairing, scopes};

/// The method a device calls to trade a pairing code for its token. It is the one method a
/// request without a token may call: a device has no token until it has called it.
pub(crate) const CLAIM: &str = "auth.pair.claim";

/// What a pairing code grants: the scopes, and the name it was made with, when it was given one.
pub type CodeGrant = (Vec<String>, Option<String>);

/// Persistence interface for paired devices and single-use pairing codes.
pub trait PairingStore: Send + Sync {
    /// Lists paired devices, newest first.
    ///
    /// # Errors
    ///
    /// Returns an error when the backing store cannot be read.
    fn paired_devices(&self) -> Result<Vec<PairedDevice>, Error>;

    /// Looks up the paired device whose token hashes to `token_hash`.
    ///
    /// # Errors
    ///
    /// Returns an error when the backing store cannot be read.
    fn device_by_token_hash(&self, token_hash: &str) -> Result<Option<PairedDevice>, Error>;

    /// Looks up the paired device with `id`.
    ///
    /// # Errors
    ///
    /// Returns an error when the backing store cannot be read.
    fn paired_device(&self, id: &str) -> Result<Option<PairedDevice>, Error>;

    /// Stores a newly paired device with the SHA-256 hex hash of its token.
    ///
    /// # Errors
    ///
    /// Returns an error when the backing store cannot be written.
    fn add_device(&self, paired: &PairedDevice, token_hash: &str) -> Result<(), Error>;

    /// Removes a paired device by `id`, returning whether it existed.
    ///
    /// # Errors
    ///
    /// Returns an error when the backing store cannot be written.
    fn remove_device(&self, id: &str) -> Result<bool, Error>;

    /// Updates a device's name and/or scopes, returning the updated record if it existed.
    ///
    /// # Errors
    ///
    /// Returns an error when the backing store cannot be written.
    fn update_device(
        &self,
        id: &str,
        name: Option<&str>,
        scopes: Option<&[String]>,
    ) -> Result<Option<PairedDevice>, Error>;

    /// Notes that a device connected at `now_ms`.
    ///
    /// # Errors
    ///
    /// Returns an error when the backing store cannot be written.
    fn touch_device(&self, id: &str, now_ms: i64) -> Result<(), Error>;

    /// Saves a pairing code, expiring any older codes at or before `now_ms`.
    ///
    /// # Errors
    ///
    /// Returns an error when the backing store cannot be written.
    fn add_pair_code(
        &self,
        code: &str,
        scopes: &[String],
        name: Option<&str>,
        expires_at_ms: i64,
        now_ms: i64,
    ) -> Result<(), Error>;

    /// Consumes a pairing code, returning its grant if it existed and had not expired by `now_ms`.
    ///
    /// # Errors
    ///
    /// Returns an error when the backing store cannot be read or written.
    fn take_pair_code(&self, code: &str, now_ms: i64) -> Result<Option<CodeGrant>, Error>;
}

/// What the server needs from the host application: a [`PairingStore`], RPC dispatch for
/// application methods, and push frame generation for touched chats.
pub trait Handler: Send + Sync {
    /// The store devices and pairing codes are kept in.
    fn pairing_store(&self) -> &dyn PairingStore;

    /// Answers `method`, whose scope was already checked.
    ///
    /// # Errors
    ///
    /// Returns the message the client shows: bad params, a method this app does not have, or
    /// what went wrong doing it.
    fn call(self: Arc<Self>, method: &str, params: &Value) -> Result<Value, Error>;

    /// The pushes that bring a client up to date after the change `touched` describes.
    fn pushes(&self, touched: &Touched) -> Vec<(&'static str, Value)>;
}

/// What changed in one chat since its last pushes.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Touched {
    pub chat_id: String,
    /// Messages that were added or changed.
    pub messages: Vec<String>,
    /// Messages that left the chat.
    pub removed: Vec<String>,
    /// The chat itself was deleted.
    pub deleted: bool,
}

impl Touched {
    /// Takes in what `other`, a later change to the same chat, touched.
    pub fn merge(&mut self, other: Self) {
        for id in other.messages {
            if !self.messages.contains(&id) {
                self.messages.push(id);
            }
        }
        for id in other.removed {
            self.messages.retain(|m| *m != id);
            if !self.removed.contains(&id) {
                self.removed.push(id);
            }
        }
        self.deleted |= other.deleted;
    }
}

/// Who a request is from.
#[derive(Debug, Clone)]
pub(crate) enum Caller {
    /// The server's own token: every scope.
    Server,
    /// A paired device, with the scopes it has now.
    Device { id: String, scopes: Vec<String> },
}

impl Caller {
    /// Whether it may call `method`.
    pub fn may_call(&self, method: &str) -> bool {
        match self {
            Self::Server => true,
            Self::Device { scopes, .. } => scopes::may_call(scopes, method),
        }
    }

    /// The paired device it is, when it is one.
    pub fn device(&self) -> Option<&str> {
        match self {
            Self::Server => None,
            Self::Device { id, .. } => Some(id),
        }
    }

    fn is_admin(&self) -> bool {
        match self {
            Self::Server => true,
            Self::Device { scopes, .. } => scopes.iter().any(|s| s == scopes::ADMIN),
        }
    }
}

/// Who `token` belongs to: the server, a paired device, or no one. A device's scopes are read
/// anew each time, so a revoked or narrowed device loses its rights on its next call.
pub(crate) fn caller(live: &Live, store: &dyn PairingStore, token: Option<&str>) -> Option<Caller> {
    let token = token?;
    if pairing::same_token(token, &live.token) {
        return Some(Caller::Server);
    }
    let paired = match store.device_by_token_hash(&pairing::hash(token)) {
        Ok(paired) => paired?,
        Err(e) => {
            log::warn!(error:% = e; "could not look up a device's token: {{error}}");
            return None;
        }
    };
    if let Err(e) = store.touch_device(&paired.device.id, now_ms()) {
        log::warn!(error:% = e; "could not note when a device was last seen: {{error}}");
    }
    Some(Caller::Device {
        id: paired.device.id,
        scopes: paired.device.scopes,
    })
}

/// Answers one request from `caller` (`None`: no valid token).
///
/// # Errors
///
/// Returns the message the client shows.
pub(crate) fn dispatch(
    live: &Live,
    backend: &Arc<dyn Handler>,
    caller: Option<&Caller>,
    method: &str,
    params: &Value,
) -> Result<Value, Error> {
    let store = backend.pairing_store();
    if method == CLAIM {
        let (token, paired) = pairing::claim(
            store,
            &live.shared.guesses,
            text(params, method, "code")?,
            params["name"].as_str().unwrap_or_default(),
            params["platform"].as_str().unwrap_or_default(),
            now_ms(),
        )?;
        return value(&Claimed {
            token,
            device: device(&paired),
        });
    }
    let Some(caller) = caller else {
        return Err("Unauthorized".into());
    };
    if !caller.may_call(method) {
        return Err(format!("This device's token may not call {method}").into());
    }
    match method {
        "auth.devices" => value(
            &store
                .paired_devices()?
                .iter()
                .map(device)
                .collect::<Vec<_>>(),
        ),
        "auth.pair.start" => {
            let scopes = match params.get("scopes") {
                None | Some(Value::Null) => Vec::new(),
                Some(list) => serde_json::from_value(list.clone())
                    .map_err(|e| format!("Bad params for {method}: {e}"))?,
            };
            let made = pairing::start(store, scopes, params["name"].as_str(), now_ms())?;
            value(&PairingCode {
                code: made.code,
                expires_at: made.expires_at_ms,
                scopes: made.scopes,
            })
        }
        "auth.devices.revoke" => {
            live.shared.revoke(store, text(params, method, "id")?)?;
            Ok(Value::Null)
        }
        "auth.devices.update" => update_device(live, store, method, params),
        "pairing.info" => value(&PairingInfo {
            urls: live.urls(),
            // A device that may not pair others has no use for the app's own token.
            token: if caller.is_admin() {
                live.token.clone()
            } else {
                String::new()
            },
            reachable: live.network,
        }),
        "events.since" => {
            // Pushes are not kept, so a client that missed any reloads instead of replaying them.
            let next = live.shared.next_seq();
            value(&EventsSince {
                floor: next,
                next,
                events: Vec::new(),
            })
        }
        _ => Arc::clone(backend).call(method, params),
    }
}

fn update_device(
    live: &Live,
    store: &dyn PairingStore,
    method: &str,
    params: &Value,
) -> Result<Value, Error> {
    let id = text(params, method, "id")?;
    let name = params["name"]
        .as_str()
        .map(str::trim)
        .filter(|n| !n.is_empty());
    let scopes: Option<Vec<String>> = match params.get("scopes") {
        None | Some(Value::Null) => None,
        Some(list) => Some(
            serde_json::from_value(list.clone())
                .map_err(|e| format!("Bad params for {method}: {e}"))?,
        ),
    };
    if let Some(scopes) = &scopes
        && !scopes::known(scopes)
    {
        return Err("A device can only be given the scopes read, write, terminal and admin".into());
    }
    let paired = store
        .update_device(id, name, scopes.as_deref())?
        .ok_or("No such device")?;
    if scopes.is_some() {
        // Its open sockets were let in with the old scopes; they reconnect with the new ones.
        live.shared.close_device(id);
    }
    value(&device(&paired))
}

/// The string param `name`.
fn text<'a>(params: &'a Value, method: &str, name: &str) -> Result<&'a str, Error> {
    params[name]
        .as_str()
        .ok_or_else(|| format!("Bad params for {method}: {name} is required").into())
}

/// A device as the wire `Device` type has it.
fn device(paired: &PairedDevice) -> Device {
    let d = &paired.device;
    Device {
        id: d.id.clone(),
        name: d.name.clone(),
        platform: d.platform.clone(),
        scopes: d.scopes.clone(),
        created_at: d.created_at_ms,
        last_seen_at: d.last_seen_ms,
        token_hint: paired.token_hint.clone(),
    }
}

/// An answer as JSON.
fn value<T: Serialize>(answer: &T) -> Result<Value, Error> {
    serde_json::to_value(answer).map_err(|e| e.to_string().into())
}
