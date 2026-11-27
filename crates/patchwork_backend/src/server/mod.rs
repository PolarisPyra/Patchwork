//! The server inside the app, which lets a phone or a browser on another computer follow and
//! drive Patchwork's chats.
//!
//! Transport, pairing, and scope enforcement live in `patchwork-rpc-server`; this module wires
//! [`Core`] and [`Store`] into it as its backend.

// Rust guideline compliant 2026-10-09

mod wire;

use std::path::PathBuf;
use std::sync::{Arc, Weak};

pub use patchwork_rpc_server::{
    DEFAULT_PORT, Device, PairCode, PairedDevice, ServerInfo, qr_modules, scopes,
};
use patchwork_rpc_server::{Backend, CodeGrant, Options, PairingStore, now_ms, pairing};

use crate::events::ChatEventEnvelope;
use crate::persistence::Store;
use crate::{Core, Error};

/// The variable that names a folder of web files to serve beside the API, such as the web app's
/// build, so a browser that opens the server's address gets the app.
const WEB_DIR_VAR: &str = "PATCHWORK_WEB_DIR";

/// Thin wrapper around [`patchwork_rpc_server::Server`] that adapts [`ChatEventEnvelope`] into
/// push updates without building work when no sockets are connected.
#[derive(Debug, Default)]
pub(crate) struct Server(patchwork_rpc_server::Server);

impl Server {
    pub(crate) fn start(&self, backend: Weak<dyn Backend>, options: Options) -> Result<u16, Error> {
        self.0.start(backend, options)
    }

    pub(crate) fn info(&self, saved_network: bool, owns_data: bool) -> ServerInfo {
        self.0.info(saved_network, owns_data)
    }

    pub(crate) fn pair_link(&self, code: &str) -> String {
        self.0.pair_link(code)
    }

    pub(crate) fn revoke(&self, store: &Store, id: &str) -> Result<(), Error> {
        self.0.revoke(store, id)
    }

    /// Passes an app event on to the open sockets.
    pub(crate) fn observe(&self, envelope: &ChatEventEnvelope) {
        if !self.0.has_sockets() {
            return;
        }
        self.0
            .touch(wire::touched_by(&envelope.chat_id, &envelope.event));
    }

    /// Pushes message `message_id` of chat `chat_id`, which was saved without an event.
    pub(crate) fn touch_message(&self, chat_id: &str, message_id: &str) {
        self.0.touch_message(chat_id, message_id);
    }
}

impl PairingStore for Store {
    fn paired_devices(&self) -> Result<Vec<PairedDevice>, Error> {
        Store::paired_devices(self)
    }

    fn device_by_token_hash(&self, token_hash: &str) -> Result<Option<PairedDevice>, Error> {
        Store::device_by_token_hash(self, token_hash)
    }

    fn paired_device(&self, id: &str) -> Result<Option<PairedDevice>, Error> {
        Store::paired_device(self, id)
    }

    fn add_device(&self, paired: &PairedDevice, token_hash: &str) -> Result<(), Error> {
        Store::add_device(self, paired, token_hash)
    }

    fn remove_device(&self, id: &str) -> Result<bool, Error> {
        Store::remove_device(self, id)
    }

    fn update_device(
        &self,
        id: &str,
        name: Option<&str>,
        scopes: Option<&[String]>,
    ) -> Result<Option<PairedDevice>, Error> {
        Store::update_device(self, id, name, scopes)
    }

    fn touch_device(&self, id: &str, now_ms: i64) -> Result<(), Error> {
        Store::touch_device(self, id, now_ms)
    }

    fn add_pair_code(
        &self,
        code: &str,
        scopes: &[String],
        name: Option<&str>,
        expires_at_ms: i64,
        now_ms: i64,
    ) -> Result<(), Error> {
        Store::add_pair_code(self, code, scopes, name, expires_at_ms, now_ms)
    }

    fn take_pair_code(&self, code: &str, now_ms: i64) -> Result<Option<CodeGrant>, Error> {
        Store::take_pair_code(self, code, now_ms)
    }
}

impl Core {
    /// Starts the server, when this is the Patchwork that owns the data folder: a second one
    /// beside it would answer for chats the first one runs. Listens on this computer only, or on
    /// the network when Settings → Devices allows it.
    pub fn start_server(self: &Arc<Self>) {
        if !self.store.owns_data() {
            log::info!("not starting the server: another Patchwork owns the data folder");
            return;
        }
        let backend: Weak<dyn Backend> = Arc::downgrade(self) as Weak<Core>;
        let options = Options {
            port: DEFAULT_PORT,
            network: self.store.settings().devices_on_network,
            web_dir: std::env::var_os(WEB_DIR_VAR).map(PathBuf::from),
        };
        // The error is kept for Settings → Devices to show, and logged where it happened.
        let _ = self.server.start(backend, options);
    }

    /// Starts the server with explicit binding options.
    ///
    /// # Errors
    ///
    /// Returns an error if another Patchwork owns the data folder or if no port can be bound.
    pub fn start_server_with(
        self: &Arc<Self>,
        port: u16,
        network: bool,
        web_dir: Option<PathBuf>,
    ) -> Result<ServerInfo, Error> {
        if !self.store.owns_data() {
            return Err("another Patchwork instance owns the data folder".into());
        }
        let backend: Weak<dyn Backend> = Arc::downgrade(self) as Weak<Core>;
        let options = Options {
            port,
            network,
            web_dir,
        };
        self.server.start(backend, options)?;
        Ok(self.server.info(network, true))
    }

    /// The running server's addresses and token.
    #[must_use]
    pub fn server_info(&self) -> ServerInfo {
        self.server.info(
            self.store.settings().devices_on_network,
            self.store.owns_data(),
        )
    }

    /// Makes a pairing code that lasts five minutes.
    ///
    /// # Errors
    /// When the code cannot be saved.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "the signature Settings → Devices was built against takes the name by value"
    )]
    pub fn pair_start(&self, name: Option<String>) -> Result<PairCode, Error> {
        // A device paired from the app can use it as the window does, but not pair others.
        let scopes = [scopes::READ, scopes::WRITE, scopes::TERMINAL]
            .map(String::from)
            .to_vec();
        let made = pairing::start(&self.store, scopes, name.as_deref(), now_ms())?;
        Ok(PairCode {
            link: self.server.pair_link(&made.code),
            code: made.code,
            expires_at_ms: made.expires_at_ms,
        })
    }

    /// The paired devices, newest first.
    ///
    /// # Errors
    /// When the database cannot be read.
    pub fn devices(&self) -> Result<Vec<Device>, Error> {
        Ok(self
            .store
            .paired_devices()?
            .into_iter()
            .map(|p| p.device)
            .collect())
    }

    /// Revokes a device's token; it is cut off at once.
    ///
    /// # Errors
    /// When the database cannot be written.
    pub fn revoke_device(&self, id: &str) -> Result<(), Error> {
        self.server.revoke(&self.store, id)
    }

    /// Renames a paired device.
    ///
    /// # Errors
    /// When the database cannot be written.
    pub fn rename_device(&self, id: &str, name: &str) -> Result<(), Error> {
        let name = name.trim();
        if name.is_empty() {
            return Err("A device needs a name".into());
        }
        self.store
            .update_device(id, Some(name), None)?
            .map(|_| ())
            .ok_or_else(|| "No such device".into())
    }

    /// Saves whether devices on the network may connect. Takes effect after a restart.
    ///
    /// # Errors
    /// When the setting cannot be saved.
    pub fn set_server_network(&self, on: bool) -> Result<(), Error> {
        self.update_settings(|s| s.devices_on_network = on)
    }
}
