//! The listening socket and the connections it takes: one thread a connection, each answering one
//! HTTP request or serving one WebSocket, and one thread that turns changes into pushes for every
//! open socket.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::io::{self, Write};
use std::net::{IpAddr, Ipv4Addr, TcpListener, TcpStream, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use patchwork_protocol::frames::{Answer, ErrorMessage, Failure, Health, PushFrame, Rejected};
use patchwork_platform::{Error, Locked};
use serde_json::Value;
use tungstenite::protocol::frame::CloseFrame;
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::protocol::{Role, WebSocketConfig};
use tungstenite::{Message, Utf8Bytes, WebSocket};

use super::http::{self, Bad, Refused};
use super::pairing::{self, Guesses};
use super::rpc::{self, Handler, CLAIM, Caller, PairingStore, Touched};
use super::{ServerInfo, now_ms};

/// The port the server listens on when it is free.
pub const DEFAULT_PORT: u16 = 7703;

/// The most connections served at once. Each has a thread; a client that opens more than this is
/// misbehaving, and the extra ones are closed.
const MAX_CONNECTIONS: usize = 64;

/// How long a connection may take to send its request before it is dropped, so a client that
/// connects and says nothing does not keep a thread.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a WebSocket waits for a frame before it looks for pushes to send.
const SOCKET_POLL: Duration = Duration::from_millis(50);

/// How long writing to a client may block before it is given up on.
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// The largest WebSocket message taken: a message with pasted files fits.
const MAX_MESSAGE: usize = 16 * 1024 * 1024;

/// How long the events of a burst, such as a streaming reply's text, are gathered into one round
/// of pushes. Short enough that a reply still reads as streaming.
const GATHER: Duration = Duration::from_millis(80);

/// How long a closing socket waits for the client to answer the close.
const CLOSE_WAIT: Duration = Duration::from_secs(2);

/// The close code a socket gets when its token is wrong or was revoked, signalling the client to
/// stop reconnecting.
const UNAUTHORIZED: u16 = 4401;

/// Binding options for starting a [`Server`].
#[derive(Debug, Clone)]
pub struct Options {
    /// The port to try first; `0` lets the system choose.
    pub port: u16,
    /// Listen on every address, rather than only this machine's loopback interface.
    pub network: bool,
    /// A folder of web files to serve alongside the API.
    pub web_dir: Option<PathBuf>,
}

/// The embedded RPC server. It does nothing until [`Server::start`].
#[derive(Debug, Default)]
pub struct Server {
    shared: Arc<Shared>,
}

/// What every connection's thread shares.
#[derive(Debug, Default)]
pub(crate) struct Shared {
    /// Where the server listens, once it does.
    bound: Mutex<Option<Bound>>,
    /// Why it could not start, when it could not.
    error: Mutex<Option<String>>,
    /// The open sockets, by a number of the server's own.
    sockets: Mutex<HashMap<u64, Socket>>,
    next_socket: AtomicU64,
    /// The sequence number of the last push.
    seq: AtomicU64,
    /// Wrong pairing codes tried lately.
    pub guesses: Guesses,
    /// Where changes go to become pushes, once the server runs.
    touches: Mutex<Option<Sender<Touched>>>,
    /// The connections being served.
    connections: AtomicUsize,
}

/// Where the server listens and the token it was started with.
#[derive(Clone)]
struct Bound {
    port: u16,
    network: bool,
    token: String,
}

impl std::fmt::Debug for Bound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bound")
            .field("port", &self.port)
            .field("network", &self.network)
            .finish_non_exhaustive()
    }
}

/// An open WebSocket, to send to.
#[derive(Debug)]
struct Socket {
    /// The paired device it is, when it is one, so revoking the device closes it.
    device: Option<String>,
    out: Sender<Outgoing>,
}

/// What a socket's thread is asked to send.
#[derive(Debug)]
enum Outgoing {
    Text(String),
    Close(u16, &'static str),
}

/// The running server, as its threads see it.
pub(crate) struct Live {
    pub shared: Arc<Shared>,
    backend: Weak<dyn Handler>,
    /// The server's own token, made fresh at start.
    pub token: String,
    /// Whether it listens on the network.
    pub network: bool,
    port: u16,
    web_dir: Option<PathBuf>,
}

impl std::fmt::Debug for Live {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Live")
            .field("port", &self.port)
            .field("network", &self.network)
            .field("web_dir", &self.web_dir)
            .finish_non_exhaustive()
    }
}

impl Live {
    /// The addresses other devices reach the server at.
    pub fn urls(&self) -> Vec<String> {
        urls(self.network, self.port)
    }
}

/// The address this computer has on its network, as other devices see it. A UDP socket
/// "connected" to an outside address sends nothing; it only makes the system pick the interface
/// it would route through.
fn lan_ip() -> Option<IpAddr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(8, 8, 8, 8), 80)).ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

fn urls(network: bool, port: u16) -> Vec<String> {
    if !network {
        return Vec::new();
    }
    lan_ip()
        .map(|ip| vec![format!("http://{ip}:{port}")])
        .unwrap_or_default()
}

impl Server {
    /// Starts listening and returns the bound port. A second call while running returns the same
    /// port without rebinding.
    ///
    /// # Errors
    ///
    /// Returns an error when no port can be bound or when worker threads cannot be spawned.
    pub fn start(&self, backend: Weak<dyn Handler>, options: Options) -> Result<u16, Error> {
        let mut bound = self.shared.bound.locked();
        if let Some(b) = bound.as_ref() {
            return Ok(b.port);
        }
        let ip = if options.network {
            Ipv4Addr::UNSPECIFIED
        } else {
            Ipv4Addr::LOCALHOST
        };
        let listener = match TcpListener::bind((ip, options.port)) {
            Err(e) if e.kind() == io::ErrorKind::AddrInUse && options.port != 0 => {
                log::warn!(
                    port = options.port;
                    "port {{port}} is taken, so the server listens on one the system chooses"
                );
                TcpListener::bind((ip, 0))
            }
            other => other,
        };
        let listener = listener.and_then(|l| l.local_addr().map(|a| (l, a.port())));
        let (listener, port) = match listener {
            Ok(found) => found,
            Err(e) => {
                log::error!(error:% = e; "the server could not listen: {{error}}");
                let message = format!("The server could not listen: {e}");
                *self.shared.error.locked() = Some(message.clone());
                return Err(message.into());
            }
        };
        let token = pairing::new_token(32);
        let live = Arc::new(Live {
            shared: Arc::clone(&self.shared),
            backend,
            token: token.clone(),
            network: options.network,
            port,
            web_dir: options.web_dir,
        });
        let (touches, changes) = mpsc::channel();
        *self.shared.touches.locked() = Some(touches);
        let pushing = Arc::clone(&live);
        let started = std::thread::Builder::new()
            .name("server-pushes".into())
            .spawn(move || push_changes(&pushing, &changes))
            .and_then(|_| {
                let accepting = Arc::clone(&live);
                std::thread::Builder::new()
                    .name("server".into())
                    .spawn(move || accept(&accepting, &listener))
            });
        if let Err(e) = started {
            let message = format!("The server could not start: {e}");
            *self.shared.error.locked() = Some(message.clone());
            return Err(message.into());
        }
        *bound = Some(Bound {
            port,
            network: options.network,
            token,
        });
        *self.shared.error.locked() = None;
        log::info!(
            port = port,
            network = options.network;
            "the server listens on port {{port}} (network: {{network}})"
        );
        Ok(port)
    }

    /// Where the server listens and its token. `saved_network` is the saved setting; `owns_data`
    /// indicates whether this instance owns the data directory.
    #[must_use]
    pub fn info(&self, saved_network: bool, owns_data: bool) -> ServerInfo {
        let Some(bound) = self.shared.bound.locked().clone() else {
            let error = if owns_data {
                self.shared
                    .error
                    .locked()
                    .clone()
                    .unwrap_or_else(|| "The server has not started.".into())
            } else {
                "Another Patchwork is running with the same data, and its server answers devices."
                    .into()
            };
            return ServerInfo {
                network: saved_network,
                error: Some(error),
                ..ServerInfo::default()
            };
        };
        ServerInfo {
            address: format!("http://127.0.0.1:{}", bound.port),
            urls: urls(bound.network, bound.port),
            token: bound.token,
            network: saved_network,
            restart_needed: saved_network != bound.network,
            error: None,
        }
    }

    /// The link that pairs a device with `code` in one step: on the network address when there
    /// is one, since that is the one a phone can open.
    #[must_use]
    pub fn pair_link(&self, code: &str) -> String {
        let Some(bound) = self.shared.bound.locked().clone() else {
            return String::new();
        };
        let base = urls(bound.network, bound.port)
            .into_iter()
            .next()
            .unwrap_or_else(|| format!("http://127.0.0.1:{}", bound.port));
        format!("{base}/pair?code={code}")
    }

    /// Forgets the device with `id` and closes its sockets.
    ///
    /// # Errors
    ///
    /// Returns an error when the backing store cannot be written.
    pub fn revoke(&self, store: &dyn PairingStore, id: &str) -> Result<(), Error> {
        self.shared.revoke(store, id)
    }

    /// Whether at least one WebSocket client is currently connected.
    #[must_use]
    pub fn has_sockets(&self) -> bool {
        !self.shared.sockets.locked().is_empty()
    }

    /// Enqueues a chat change to be pushed to open sockets.
    pub fn touch(&self, touched: Touched) {
        self.shared.touch(touched);
    }
}

impl Shared {
    fn touch(&self, touched: Touched) {
        if let Some(touches) = self.touches.locked().as_ref() {
            // The pusher only stops with the process.
            let _ = touches.send(touched);
        }
    }

    /// The sequence number the next push gets.
    pub fn next_seq(&self) -> u64 {
        self.seq.load(Ordering::SeqCst) + 1
    }

    /// Forgets the device with `id` and closes its sockets at once.
    pub fn revoke(&self, store: &dyn PairingStore, id: &str) -> Result<(), Error> {
        if store.remove_device(id)? {
            log::info!(device:% = id; "revoked device {{device}}");
        }
        self.close_device(id);
        Ok(())
    }

    /// Closes the sockets of the device with `id`, telling them their token no longer works.
    pub fn close_device(&self, id: &str) {
        for socket in self.sockets.locked().values() {
            if socket.device.as_deref() == Some(id) {
                let _ = socket
                    .out
                    .send(Outgoing::Close(UNAUTHORIZED, "Unauthorized"));
            }
        }
    }

    /// Sends a push to every open socket.
    fn broadcast(&self, push: &str, data: &Value) {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let frame = frame_text(&PushFrame {
            push: push.to_string(),
            data: data.clone(),
            seq,
        });
        for socket in self.sockets.locked().values() {
            // A socket that is gone is removed by its own thread.
            let _ = socket.out.send(Outgoing::Text(frame.clone()));
        }
    }
}

/// Counts a connection while it is served.
struct Counted<'a>(&'a AtomicUsize);

impl Drop for Counted<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Takes connections for as long as the process runs.
fn accept(live: &Arc<Live>, listener: &TcpListener) {
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(stream) => stream,
            Err(e) => {
                log::debug!(error:% = e; "the server could not take a connection: {{error}}");
                continue;
            }
        };
        if live.shared.connections.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            live.shared.connections.fetch_sub(1, Ordering::SeqCst);
            log::warn!("the server is serving too many connections, so it closed a new one");
            continue;
        }
        let serving = Arc::clone(live);
        let spawned = std::thread::Builder::new()
            .name("server-connection".into())
            .spawn(move || {
                let _counted = Counted(&serving.shared.connections);
                serve(&serving, stream);
            });
        if let Err(e) = spawned {
            live.shared.connections.fetch_sub(1, Ordering::SeqCst);
            log::warn!(error:% = e; "the server could not serve a connection: {{error}}");
        }
    }
}

/// Answers the one request on `stream`.
fn serve(live: &Arc<Live>, mut stream: TcpStream) {
    // Without timeouts a client that stops partway keeps the thread.
    let _ = stream.set_read_timeout(Some(REQUEST_TIMEOUT));
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    let _ = stream.set_nodelay(true);
    let request = match http::read_request(&mut stream) {
        Ok(request) => request,
        Err(Bad::Gone) => return,
        Err(Bad::Status(status, reason)) => {
            http::respond(&mut stream, status, reason, &[], b"");
            return;
        }
    };
    let Some(backend) = live.backend.upgrade() else {
        http::respond(&mut stream, 503, "Service Unavailable", &[], b"");
        return;
    };
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/health") => http::respond_json(
            &mut stream,
            200,
            &Health {
                ok: true,
                version: patchwork_platform::VERSION.to_string(),
                seq: live.shared.next_seq(),
            },
        ),
        ("POST", "/rpc") => rpc_over_http(live, &backend, &mut stream, &request),
        ("GET", "/ws") if request.is_upgrade() => {
            // The socket looks the backend up for each request, so it does not keep it alive.
            drop(backend);
            websocket(live, stream, request);
        }
        ("GET", "/pair") => http::respond(
            &mut stream,
            200,
            "OK",
            &http::PAGE_HEADERS,
            http::pair_form(request.param("code"), None).as_bytes(),
        ),
        ("POST", "/pair") => pair_over_form(live, backend.pairing_store(), &mut stream, &request),
        ("GET" | "HEAD", _) => web_file(live, &mut stream, &request),
        _ => http::respond(&mut stream, 405, "Method Not Allowed", &[], b""),
    }
}

/// One request over `POST /rpc`: 401 without a valid token, and otherwise 200 with the result or
/// the error.
fn rpc_over_http(
    live: &Live,
    backend: &Arc<dyn Handler>,
    stream: &mut TcpStream,
    request: &http::Request,
) {
    let Ok(frame) = serde_json::from_slice::<Value>(&request.body) else {
        http::respond_json(stream, 200, &failure(Value::Null, "Not JSON"));
        return;
    };
    let method = frame["method"].as_str().unwrap_or_default();
    let caller = rpc::caller(live, backend.pairing_store(), request.token());
    if caller.is_none() && method != CLAIM {
        http::respond_json(
            stream,
            401,
            &Rejected {
                error: ErrorMessage::new("Unauthorized"),
            },
        );
        return;
    }
    let params = params_of(&frame);
    match rpc::dispatch(live, backend, caller.as_ref(), method, &params) {
        Ok(result) => {
            let id = frame.get("id").cloned().unwrap_or(Value::Null);
            http::respond_json(stream, 200, &Answer { id, result });
        }
        Err(e) => http::respond_json(stream, 200, &failure(Value::Null, &e.to_string())),
    }
}

/// A frame's params; an empty object when it has none.
fn params_of(frame: &Value) -> Value {
    match frame.get("params") {
        None | Some(Value::Null) => Value::Object(serde_json::Map::new()),
        Some(params) => params.clone(),
    }
}

/// The form on the pairing page, sent: pairs this browser and, when the web app is served, opens
/// it signed in.
fn pair_over_form(
    live: &Live,
    store: &dyn PairingStore,
    stream: &mut TcpStream,
    request: &http::Request,
) {
    let form: HashMap<String, String> = url::form_urlencoded::parse(&request.body)
        .into_owned()
        .collect();
    let field = |name: &str| form.get(name).map(String::as_str).unwrap_or_default();
    match pairing::claim(
        store,
        &live.shared.guesses,
        field("code"),
        field("name"),
        "web",
        now_ms(),
    ) {
        Ok((token, paired)) if live.web_dir.is_some() => {
            // The token is base64url, so it goes into the address as it is.
            let location = format!("/?token={token}");
            let mut headers = http::PAGE_HEADERS.to_vec();
            headers.push(("Location", &location));
            let body = http::paired_page(&paired.device.name, &token);
            http::respond(stream, 303, "See Other", &headers, body.as_bytes());
        }
        Ok((token, paired)) => {
            let body = http::paired_page(&paired.device.name, &token);
            http::respond(stream, 200, "OK", &http::PAGE_HEADERS, body.as_bytes());
        }
        Err(e) => {
            let body = http::pair_form(None, Some(&e.to_string()));
            http::respond(
                stream,
                400,
                "Bad Request",
                &http::PAGE_HEADERS,
                body.as_bytes(),
            );
        }
    }
}

/// A file of the web app, when one is served.
fn web_file(live: &Live, stream: &mut TcpStream, request: &http::Request) {
    let Some(root) = &live.web_dir else {
        http::respond(stream, 404, "Not Found", &[], b"");
        return;
    };
    match http::web_file(root, &request.path) {
        Ok(file) => match std::fs::read(&file) {
            Ok(body) => {
                let body = if request.method == "HEAD" {
                    Vec::new()
                } else {
                    body
                };
                http::respond(
                    stream,
                    200,
                    "OK",
                    &[("Content-Type", http::mime(&file))],
                    &body,
                );
            }
            Err(_) => http::respond(stream, 404, "Not Found", &[], b""),
        },
        Err(Refused::Outside) => {
            log::warn!("refused a request for a file outside the web folder");
            http::respond(stream, 403, "Forbidden", &[], b"");
        }
        Err(Refused::Missing) => http::respond(stream, 404, "Not Found", &[], b""),
    }
}

/// Takes the socket out of the open ones when its thread ends.
struct Registered<'a> {
    shared: &'a Shared,
    id: u64,
}

impl Drop for Registered<'_> {
    fn drop(&mut self) {
        self.shared.sockets.locked().remove(&self.id);
    }
}

/// Serves a WebSocket: requests in, answers and pushes out, until either side closes it.
fn websocket(live: &Live, mut stream: TcpStream, request: http::Request) {
    let Some(key) = request.header("sec-websocket-key") else {
        http::respond(&mut stream, 400, "Bad Request", &[], b"");
        return;
    };
    let accept = tungstenite::handshake::derive_accept_key(key.as_bytes());
    let handshake = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    if stream.write_all(handshake.as_bytes()).is_err() {
        return;
    }
    // A short read timeout lets the thread look for pushes between the client's frames.
    let _ = stream.set_read_timeout(Some(SOCKET_POLL));
    let caller = live
        .backend
        .upgrade()
        .and_then(|backend| rpc::caller(live, backend.pairing_store(), request.token()));
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE))
        .max_frame_size(Some(MAX_MESSAGE));
    let mut ws = WebSocket::from_partially_read(stream, request.rest, Role::Server, Some(config));
    // Accept and then close with 4401 so clients see why the connection was rejected.
    let Some(caller) = caller else {
        close(&mut ws, UNAUTHORIZED, "Unauthorized");
        return;
    };
    let (out, outgoing) = mpsc::channel();
    let id = live.shared.next_socket.fetch_add(1, Ordering::SeqCst);
    live.shared.sockets.locked().insert(
        id,
        Socket {
            device: caller.device().map(String::from),
            out,
        },
    );
    let _registered = Registered {
        shared: &live.shared,
        id,
    };
    loop {
        loop {
            match outgoing.try_recv() {
                Ok(Outgoing::Text(text)) => {
                    if ws.send(Message::text(text)).is_err() {
                        return;
                    }
                }
                Ok(Outgoing::Close(code, reason)) => {
                    close(&mut ws, code, reason);
                    return;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        match ws.read() {
            Ok(Message::Text(text)) => {
                let answer = answer(live, &caller, &text);
                if ws.send(Message::text(answer)).is_err() {
                    return;
                }
            }
            Ok(Message::Close(_)) => {
                // Reading on sends the close back and ends with the connection closed.
                drain(&mut ws);
                return;
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if waited(&e) => {}
            Err(_) => return,
        }
    }
}

/// Whether a read only ran out of time, rather than failing.
fn waited(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

/// Closes a socket with `code` and waits briefly for the client to answer.
fn close(ws: &mut WebSocket<TcpStream>, code: u16, reason: &'static str) {
    let _ = ws.close(Some(CloseFrame {
        code: CloseCode::from(code),
        reason: Utf8Bytes::from_static(reason),
    }));
    drain(ws);
}

/// Reads until the socket is closed or `CLOSE_WAIT` passes, which sends what is still queued.
fn drain(ws: &mut WebSocket<TcpStream>) {
    let deadline = Instant::now() + CLOSE_WAIT;
    while Instant::now() < deadline {
        match ws.read() {
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if waited(&e) => {}
            Err(_) => return,
        }
    }
}

/// The answer to one frame from a socket.
fn answer(live: &Live, caller: &Caller, text: &str) -> String {
    let Ok(frame) = serde_json::from_str::<Value>(text) else {
        return frame_text(&failure(Value::Null, "Not JSON"));
    };
    let id = frame.get("id").cloned().unwrap_or(Value::Null);
    let failed = |message: &str| frame_text(&failure(id.clone(), message));
    let Some(backend) = live.backend.upgrade() else {
        return failed("Patchwork is closing");
    };
    // A device's rights are read anew, so one revoked or narrowed since it connected cannot go
    // on as it was.
    let caller = match caller {
        Caller::Server => Caller::Server,
        Caller::Device { id, .. } => match backend.pairing_store().paired_device(id) {
            Ok(Some(paired)) => Caller::Device {
                id: paired.device.id,
                scopes: paired.device.scopes,
            },
            _ => return failed("Unauthorized"),
        },
    };
    let method = frame["method"].as_str().unwrap_or_default();
    match rpc::dispatch(live, &backend, Some(&caller), method, &params_of(&frame)) {
        Ok(result) => frame_text(&Answer { id, result }),
        Err(e) => failed(&e.to_string()),
    }
}

/// Turns changes into pushes, gathering each burst into one round.
fn push_changes(live: &Live, changes: &Receiver<Touched>) {
    while let Ok(first) = changes.recv() {
        let deadline = Instant::now() + GATHER;
        let mut gathered: Vec<Touched> = vec![first];
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            match changes.recv_timeout(left) {
                Ok(touched) => gathered.push(touched),
                Err(_) => break,
            }
        }
        let mut by_chat: Vec<Touched> = Vec::new();
        for touched in gathered {
            match by_chat.iter_mut().find(|t| t.chat_id == touched.chat_id) {
                Some(seen) => seen.merge(touched),
                None => by_chat.push(touched),
            }
        }
        let Some(backend) = live.backend.upgrade() else {
            return;
        };
        for touched in &by_chat {
            for (push, data) in backend.pushes(touched) {
                live.shared.broadcast(push, &data);
            }
        }
    }
}

/// A failed request's frame.
fn failure(id: Value, message: &str) -> Failure {
    Failure {
        id,
        error: ErrorMessage::new(message),
    }
}

/// A frame as the text sent on a socket.
fn frame_text<T: serde::Serialize>(frame: &T) -> String {
    // Frames are maps of strings and JSON values, which always serialize.
    serde_json::to_string(frame).unwrap_or_default()
}
