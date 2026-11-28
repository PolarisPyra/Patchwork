//! A transport over a Patchwork's socket, as the phone reaches the desktop.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use patchwork_protocol::devices::Claimed;
use patchwork_protocol::frames::{Answer, Failure, PushFrame, Rejected};
use patchwork_protocol::pushes::Push;
use serde_json::{Value, json};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};
use url::Url;

use super::{Transport, ClientError, PushSink};

/// The close code a server sends when it refuses a token.
const UNAUTHORIZED: u16 = 4401;
/// How long the socket's thread waits for a frame before it looks for calls to send.
const POLL: Duration = Duration::from_millis(50);
/// How long a write, or an HTTP request, may take.
const IO_TIMEOUT: Duration = Duration::from_secs(15);
/// The method a new device claims a pairing code with, the one a request without a token may
/// call.
const CLAIM: &str = "auth.pair.claim";

/// The socket's stream, plain or over TLS.
type Socket = WebSocket<MaybeTlsStream<TcpStream>>;
/// The sinks pushes are handed to.
type Sinks = Arc<Mutex<Vec<PushSink>>>;

/// A call waiting to be sent, and where its answer goes.
struct Call {
    method: String,
    params: Value,
    reply: Sender<Result<Value, ClientError>>,
}

/// A Patchwork reached over its socket at `/ws`.
///
/// A thread of its own owns the socket: it sends calls, matches answers to them, and hands
/// pushes to the sinks. Dropping the transport closes the socket.
pub struct RemoteTransport {
    calls: Mutex<Sender<Call>>,
    sinks: Sinks,
}

impl std::fmt::Debug for RemoteTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteTransport").finish_non_exhaustive()
    }
}

impl RemoteTransport {
    /// Connects to the Patchwork at `base_url`, such as `http://192.168.1.5:7703`, with a
    /// paired device's `token`.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::InvalidUrl`] for an address that is not one, or why the socket
    /// did not open.
    pub fn connect(base_url: &str, token: &str) -> Result<Self, ClientError> {
        let url = socket_url(base_url, token)?;
        let (socket, _) = tungstenite::connect(url.as_str())?;
        if let Some(stream) = tcp_stream(&socket) {
            stream.set_read_timeout(Some(POLL))?;
            stream.set_write_timeout(Some(IO_TIMEOUT))?;
        }
        let (calls, received) = mpsc::channel();
        let sinks: Sinks = Arc::default();
        let pushes = Arc::clone(&sinks);
        std::thread::Builder::new()
            .name("patchwork-remote".into())
            .spawn(move || run(socket, &received, &pushes))?;
        Ok(Self {
            calls: Mutex::new(calls),
            sinks,
        })
    }

    /// Claims pairing `code` at the Patchwork at `base_url`, as the device `name` on
    /// `platform` (such as `android`). The answer holds the device's token, shown only once.
    ///
    /// # Errors
    ///
    /// Returns the Patchwork's message for a wrong or expired code, or why it could not be
    /// reached.
    pub fn pair(
        base_url: &str,
        code: &str,
        name: &str,
        platform: &str,
    ) -> Result<Claimed, ClientError> {
        let base = http_base(base_url)?;
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(IO_TIMEOUT))
            .http_status_as_error(false)
            .build()
            .into();
        let payload = json!({
            "id": 1,
            "method": CLAIM,
            "params": { "code": code, "name": name, "platform": platform },
        });
        let mut response = agent
            .post(&format!("{base}/rpc"))
            .header("Content-Type", "application/json")
            .send(payload.to_string())?;
        let status = response.status().as_u16();
        let body = response.body_mut().read_to_string()?;
        if status == 401 {
            let message = serde_json::from_str::<Rejected>(&body)
                .map_or_else(|_| "Unauthorized".to_string(), |r| r.error.message);
            return Err(ClientError::Unauthorized(message));
        }
        if status != 200 {
            return Err(ClientError::HttpStatus { status, body });
        }
        let raw: Value = serde_json::from_str(&body)?;
        if raw.get("error").is_some() {
            let failure: Failure = serde_json::from_value(raw)?;
            return Err(ClientError::Rpc(failure.error.message));
        }
        let answer: Answer = serde_json::from_value(raw)?;
        Ok(serde_json::from_value(answer.result)?)
    }
}

impl Transport for RemoteTransport {
    fn call_raw(&self, method: &str, params: Value) -> Result<Value, ClientError> {
        let (reply, answer) = mpsc::channel();
        let call = Call {
            method: method.to_string(),
            params,
            reply,
        };
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .send(call)
            .map_err(|_closed| ClientError::Closed(None))?;
        answer.recv().map_err(|_closed| ClientError::Closed(None))?
    }

    fn subscribe(&self, sink: PushSink) {
        self.sinks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(sink);
    }
}

/// Runs the socket until it closes or the transport is dropped.
fn run(mut socket: Socket, calls: &Receiver<Call>, sinks: &Sinks) {
    let mut waiting: HashMap<u64, Sender<Result<Value, ClientError>>> = HashMap::new();
    let mut next_id = 1_u64;
    loop {
        loop {
            match calls.try_recv() {
                Ok(call) => {
                    let id = next_id;
                    next_id += 1;
                    let frame = json!({ "id": id, "method": call.method, "params": call.params });
                    match socket.send(Message::text(frame.to_string())) {
                        Ok(()) => {
                            waiting.insert(id, call.reply);
                        }
                        Err(e) => {
                            let _ = call.reply.send(Err(e.into()));
                        }
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    let _ = socket.close(None);
                    return;
                }
            }
        }
        let closed = match socket.read() {
            Ok(Message::Text(text)) => {
                take_frame(text.as_str(), &mut waiting, sinks);
                None
            }
            Ok(Message::Close(frame)) => Some(match frame {
                Some(f) if u16::from(f.code) == UNAUTHORIZED => {
                    ClientError::Unauthorized(f.reason.to_string())
                }
                f => ClientError::Closed(f.map(|f| u16::from(f.code))),
            }),
            Ok(_) => None,
            Err(e) if timed_out(&e) => None,
            Err(e) => Some(e.into()),
        };
        if let Some(error) = closed {
            log::info!(error:% = error; "the Patchwork connection closed: {{error}}");
            for (_, reply) in waiting.drain() {
                let _ = reply.send(Err(ClientError::Closed(None)));
            }
            return;
        }
    }
}

/// Hands an answer to its call, or a push to the sinks.
fn take_frame(
    text: &str,
    waiting: &mut HashMap<u64, Sender<Result<Value, ClientError>>>,
    sinks: &Sinks,
) {
    let Ok(raw) = serde_json::from_str::<Value>(text) else {
        return;
    };
    if raw.get("push").is_some() {
        let Ok(frame) = serde_json::from_value::<PushFrame>(raw) else {
            return;
        };
        if let Ok(Some(push)) = Push::from_frame(&frame.push, frame.data) {
            for sink in sinks.lock().unwrap_or_else(PoisonError::into_inner).iter() {
                sink(push.clone());
            }
        }
        return;
    }
    let id = raw.get("id").and_then(Value::as_u64);
    let Some(reply) = id.and_then(|id| waiting.remove(&id)) else {
        return;
    };
    let answer = if raw.get("error").is_some() {
        serde_json::from_value::<Failure>(raw)
            .map_err(ClientError::from)
            .and_then(|f| Err(ClientError::Rpc(f.error.message)))
    } else {
        serde_json::from_value::<Answer>(raw)
            .map(|a| a.result)
            .map_err(ClientError::from)
    };
    let _ = reply.send(answer);
}

/// The base address without a trailing slash, checked to be `http` or `https`.
fn http_base(base_url: &str) -> Result<String, ClientError> {
    let url =
        Url::parse(base_url).map_err(|e| ClientError::InvalidUrl(format!("{base_url}: {e}")))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(ClientError::InvalidUrl(format!(
            "unsupported scheme `{}`",
            url.scheme()
        )));
    }
    Ok(base_url.trim_end_matches('/').to_string())
}

/// The socket's address for `base_url`, with `token`.
fn socket_url(base_url: &str, token: &str) -> Result<Url, ClientError> {
    let mut url =
        Url::parse(base_url).map_err(|e| ClientError::InvalidUrl(format!("{base_url}: {e}")))?;
    let scheme = match url.scheme() {
        "http" | "ws" => "ws",
        "https" | "wss" => "wss",
        other => {
            return Err(ClientError::InvalidUrl(format!(
                "unsupported scheme `{other}`"
            )));
        }
    };
    url.set_scheme(scheme)
        .map_err(|()| ClientError::InvalidUrl(base_url.to_string()))?;
    let path = url.path().trim_end_matches('/');
    if !path.ends_with("/ws") {
        url.set_path(&format!("{path}/ws"));
    }
    url.query_pairs_mut().append_pair("token", token);
    Ok(url)
}

/// The TCP stream under the socket, for its timeouts.
fn tcp_stream(socket: &Socket) -> Option<&TcpStream> {
    match socket.get_ref() {
        MaybeTlsStream::Plain(stream) => Some(stream),
        MaybeTlsStream::Rustls(stream) => Some(stream.get_ref()),
        _ => None,
    }
}

/// Whether a read stopped only because no frame came in time.
fn timed_out(error: &tungstenite::Error) -> bool {
    matches!(
        error,
        tungstenite::Error::Io(io)
            if matches!(io.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
    )
}
