//! The server inside the app, which lets a phone, a browser, or another computer follow and
//! drive Patchwork's chats.
//!
//! Clients speak JSON requests over `POST /rpc` or a WebSocket at `/ws`, and receive push frames
//! over the socket as chats change. Every connection carries a token: the server's own token
//! (generated on start) or one a paired device received via pairing, restricted to its scopes.
//!
//! - `listener`, `http`: the sockets, and HTTP and WebSocket framing.
//! - `rpc`: who is calling, pairing and device methods, and the [`Handler`] the app answers with.
//! - `pairing`, `scopes`: pairing codes and tokens, and what each scope may call.
//! - `requests`: the app's methods, answered by the core.
//! - `events`: the pushes a chat event becomes.
//! - `wire`: the core's chats and messages in the protocol's shapes.

// Rust guideline compliant 2026-10-09

mod events;
mod http;
mod listener;
pub mod pairing;
mod requests;
mod rpc;
pub mod scopes;
mod wire;

use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex, Once, Weak};

use patchwork_platform::Locked;
use patchwork_protocol::pushes::Push;

pub use listener::{DEFAULT_PORT, Options};
pub use rpc::{CodeGrant, Handler, PairingStore, Touched};

pub use patchwork_core::persistence::{Device, PairedDevice};
use patchwork_core::Core;
use patchwork_core::events::Signal;
use patchwork_core::persistence::Store;
use patchwork_platform::Error;

/// Status and connection details for a running RPC server.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerInfo {
    /// The loopback address this machine reaches the server at, such as `http://127.0.0.1:7703`.
    pub address: String,
    /// The addresses other devices on the network reach it at; empty while network access is off.
    pub urls: Vec<String>,
    /// The token every admin connection must carry.
    pub token: String,
    /// Whether devices on the network may connect (the saved setting).
    pub network: bool,
    /// Whether the saved network setting differs from what the running server listens on.
    pub restart_needed: bool,
    /// Why the server is not running, when it is not.
    pub error: Option<String>,
}

/// A pairing code a new device types or scans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairCode {
    /// The code, such as `K7M-QX2`.
    pub code: String,
    /// When it stops working, in milliseconds since the Unix epoch.
    pub expires_at_ms: i64,
    /// The link that pairs a device in one step, such as `http://192.168.1.5:7703/pair?code=K7M-QX2`.
    pub link: String,
}

/// Current time in milliseconds since the Unix epoch, as device tables store it.
#[must_use]
pub fn now_ms() -> i64 {
    i64::try_from(patchwork_platform::now_ms()).unwrap_or(i64::MAX)
}

/// The QR code for `text`, as rows of dark (`true`) and light modules, without the quiet zone.
/// Returns `None` when the text is too long for a QR code.
#[must_use]
pub fn qr_modules(text: &str) -> Option<Vec<Vec<bool>>> {
    // Medium correction survives a smudged or glary screen and still fits a pairing link in a
    // small code.
    let qr = qrcodegen::QrCode::encode_text(text, qrcodegen::QrCodeEcc::Medium).ok()?;
    let size = qr.size();
    Some(
        (0..size)
            .map(|y| (0..size).map(|x| qr.get_module(x, y)).collect())
            .collect(),
    )
}

/// The variable that names a folder of web files to serve beside the API, such as the web app's
/// build, so a browser that opens the server's address gets the app.
const WEB_DIR_VAR: &str = "PATCHWORK_WEB_DIR";

/// What a local client is handed each push with: one that runs in this process, without a
/// socket between it and the core.
pub type PushSink = Box<dyn Fn(&Push) + Send + Sync>;

/// The listener, with the local clients that take pushes in-process. Turns each chat event into
/// pushes, without building them when nobody listens.
#[derive(Default)]
struct Server {
    listener: listener::Server,
    local: Mutex<Vec<PushSink>>,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("listener", &self.listener)
            .field("local", &self.local.locked().len())
            .finish()
    }
}

impl Server {
    fn start(&self, handler: Weak<dyn Handler>, options: Options) -> Result<u16, Error> {
        self.listener.start(handler, options)
    }

    fn info(&self, saved_network: bool, owns_data: bool) -> ServerInfo {
        self.listener.info(saved_network, owns_data)
    }

    fn pair_link(&self, code: &str) -> String {
        self.listener.pair_link(code)
    }

    fn revoke(&self, store: &Store, id: &str) -> Result<(), Error> {
        self.listener.revoke(store, id)
    }

    /// Passes what the core signals on to the open sockets and the local clients: a chat event,
    /// or a message the window saved without one.
    fn observe(&self, core: &Core, signal: &Signal<'_>) {
        let touched = match *signal {
            Signal::Event(envelope) => events::touched_by(&envelope.chat_id, &envelope.event),
            Signal::MessageAdded {
                chat_id,
                message_id,
            } => Touched {
                chat_id: chat_id.to_string(),
                messages: vec![message_id.to_string()],
                ..Touched::default()
            },
        };
        self.push(core, touched);
    }

    /// Sends what `touched` changed to the local clients now, and to the sockets through the
    /// listener's pusher, which gathers changes that come close together.
    fn push(&self, core: &Core, touched: Touched) {
        {
            let local = self.local.locked();
            if !local.is_empty() {
                for push in events::client_pushes(core, &touched) {
                    for sink in local.iter() {
                        sink(&push);
                    }
                }
            }
        }
        if self.listener.has_sockets() {
            self.listener.touch(touched);
        }
    }
}

/// The one server in this process, which every core's methods below share.
static SERVER: LazyLock<Server> = LazyLock::new(Server::default);

/// Whether the server already hears the core's signals, so a second start adds no second observer.
static OBSERVING: Once = Once::new();

/// Has `core` hand the server its signals, once.
fn observe(core: &Core) {
    OBSERVING.call_once(|| {
        core.observe(Box::new(|core, signal| SERVER.observe(core, signal)));
    });
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

/// The server's methods on the core: starting it, its addresses, and pairing devices.
pub trait CoreServer {
    /// Starts the server, when this is the Patchwork that owns the data folder: a second one
    /// beside it would answer for chats the first one runs. Listens on this computer only, or on
    /// the network when Settings → Devices allows it.
    fn start_server(self: &Arc<Self>);

    /// Starts the server with explicit binding options.
    ///
    /// # Errors
    ///
    /// Returns an error if another Patchwork owns the data folder or if no port can be bound.
    fn start_server_with(
        self: &Arc<Self>,
        port: u16,
        network: bool,
        web_dir: Option<PathBuf>,
    ) -> Result<ServerInfo, Error>;

    /// Hands `sink` every push from now on, as a client in this process takes them.
    fn subscribe_pushes(&self, sink: PushSink);

    /// The running server's addresses and token.
    #[must_use]
    fn server_info(&self) -> ServerInfo;

    /// Makes a pairing code that lasts five minutes.
    ///
    /// # Errors
    /// When the code cannot be saved.
    fn pair_start(&self, name: Option<String>) -> Result<PairCode, Error>;

    /// The paired devices, newest first.
    ///
    /// # Errors
    /// When the database cannot be read.
    fn devices(&self) -> Result<Vec<Device>, Error>;

    /// Revokes a device's token; it is cut off at once.
    ///
    /// # Errors
    /// When the database cannot be written.
    fn revoke_device(&self, id: &str) -> Result<(), Error>;

    /// Renames a paired device.
    ///
    /// # Errors
    /// When the database cannot be written.
    fn rename_device(&self, id: &str, name: &str) -> Result<(), Error>;

    /// Saves whether devices on the network may connect. Takes effect after a restart.
    ///
    /// # Errors
    /// When the setting cannot be saved.
    fn set_server_network(&self, on: bool) -> Result<(), Error>;
}

impl CoreServer for Core {
    /// Starts the server, when this is the Patchwork that owns the data folder: a second one
    /// beside it would answer for chats the first one runs. Listens on this computer only, or on
    /// the network when Settings → Devices allows it.
    fn start_server(self: &Arc<Self>) {
        if !self.store().owns_data() {
            log::info!("not starting the server: another Patchwork owns the data folder");
            return;
        }
        observe(self);
        let backend: Weak<dyn Handler> = Arc::downgrade(self) as Weak<Core>;
        let options = Options {
            port: DEFAULT_PORT,
            network: self.store().settings().devices_on_network,
            web_dir: std::env::var_os(WEB_DIR_VAR).map(PathBuf::from),
        };
        // The error is kept for Settings → Devices to show, and logged where it happened.
        let _ = SERVER.start(backend, options);
    }

    /// Starts the server with explicit binding options.
    ///
    /// # Errors
    ///
    /// Returns an error if another Patchwork owns the data folder or if no port can be bound.
    fn start_server_with(
        self: &Arc<Self>,
        port: u16,
        network: bool,
        web_dir: Option<PathBuf>,
    ) -> Result<ServerInfo, Error> {
        if !self.store().owns_data() {
            return Err("another Patchwork instance owns the data folder".into());
        }
        observe(self);
        let backend: Weak<dyn Handler> = Arc::downgrade(self) as Weak<Core>;
        let options = Options {
            port,
            network,
            web_dir,
        };
        SERVER.start(backend, options)?;
        Ok(SERVER.info(network, true))
    }

    /// Hands `sink` every push from now on, as a client in this process takes them.
    fn subscribe_pushes(&self, sink: PushSink) {
        observe(self);
        SERVER.local.locked().push(sink);
    }

    /// The running server's addresses and token.
    fn server_info(&self) -> ServerInfo {
        SERVER.info(
            self.store().settings().devices_on_network,
            self.store().owns_data(),
        )
    }

    /// Makes a pairing code that lasts five minutes.
    ///
    /// # Errors
    /// When the code cannot be saved.
    fn pair_start(&self, name: Option<String>) -> Result<PairCode, Error> {
        // A device paired from the app can use it as the window does, but not pair others.
        let scopes = [scopes::READ, scopes::WRITE, scopes::TERMINAL]
            .map(String::from)
            .to_vec();
        let made = pairing::start(self.store(), scopes, name.as_deref(), now_ms())?;
        Ok(PairCode {
            link: SERVER.pair_link(&made.code),
            code: made.code,
            expires_at_ms: made.expires_at_ms,
        })
    }

    /// The paired devices, newest first.
    ///
    /// # Errors
    /// When the database cannot be read.
    fn devices(&self) -> Result<Vec<Device>, Error> {
        Ok(self
            .store()
            .paired_devices()?
            .into_iter()
            .map(|p| p.device)
            .collect())
    }

    /// Revokes a device's token; it is cut off at once.
    ///
    /// # Errors
    /// When the database cannot be written.
    fn revoke_device(&self, id: &str) -> Result<(), Error> {
        SERVER.revoke(self.store(), id)
    }

    /// Renames a paired device.
    ///
    /// # Errors
    /// When the database cannot be written.
    fn rename_device(&self, id: &str, name: &str) -> Result<(), Error> {
        let name = name.trim();
        if name.is_empty() {
            return Err("A device needs a name".into());
        }
        self.store()
            .update_device(id, Some(name), None)?
            .map(|_| ())
            .ok_or_else(|| "No such device".into())
    }

    /// Saves whether devices on the network may connect. Takes effect after a restart.
    ///
    /// # Errors
    /// When the setting cannot be saved.
    fn set_server_network(&self, on: bool) -> Result<(), Error> {
        self.update_settings(|s| s.devices_on_network = on)
    }
}
