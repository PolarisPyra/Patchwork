//! WebSocket client for `/ws`: bidirectional RPC calls and live push frame streaming.

// Rust guideline compliant 2026-10-09

use std::collections::VecDeque;
use std::fmt;
use std::io::ErrorKind;
use std::net::TcpStream;
use std::time::Duration;

use patchwork_rpc_protocol::Method;
use patchwork_rpc_protocol::frames::{Answer, Failure, PushFrame};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};
use url::Url;

use crate::ClientError;

const UNAUTHORIZED_CLOSE_CODE: u16 = 4401;
const DEFAULT_IO_TIMEOUT: Duration = Duration::from_secs(15);

/// Connected WebSocket session to a Patchwork RPC server.
///
/// Any [`PushFrame`] that arrives while waiting for an RPC response is buffered in order and
/// returned by [`WsClient::next_push`] / [`WsClient::try_next_push`].
pub struct WsClient {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    pending_pushes: VecDeque<PushFrame>,
    next_id: u64,
}

impl fmt::Debug for WsClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WsClient")
            .field("pending_pushes", &self.pending_pushes.len())
            .field("next_id", &self.next_id)
            .finish_non_exhaustive()
    }
}

impl WsClient {
    /// Connects to a Patchwork server's `/ws` endpoint.
    ///
    /// Accepts `http://`, `https://`, `ws://`, or `wss://` base URLs and appends `/ws?token=...`.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::InvalidUrl`] if `base_url` is malformed or has an unsupported
    /// scheme, or [`ClientError::WebSocket`] if the handshake fails.
    pub fn connect(base_url: &str, token: &str) -> Result<Self, ClientError> {
        let ws_url = build_ws_url(base_url, token)?;
        let (socket, _) = tungstenite::connect(ws_url.as_str())?;
        set_timeouts(&socket, Some(DEFAULT_IO_TIMEOUT))?;
        Ok(Self {
            socket,
            pending_pushes: VecDeque::new(),
            next_id: 1,
        })
    }

    /// Sends a raw RPC request over the WebSocket and waits for its matching answer frame.
    ///
    /// Push frames received while awaiting the answer are queued for [`Self::next_push`].
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Unauthorized`] if the server closes with code 4401,
    /// [`ClientError::Rpc`] on an RPC error frame, [`ClientError::Closed`] if closed, or a
    /// transport/serialization error.
    pub fn call_raw(&mut self, method: &str, params: &Value) -> Result<Value, ClientError> {
        let id = self.next_id;
        self.next_id += 1;
        let payload = json!({
            "id": id,
            "method": method,
            "params": params,
        });
        self.socket.send(Message::text(payload.to_string()))?;
        set_read_timeout(&self.socket, Some(DEFAULT_IO_TIMEOUT))?;
        loop {
            let text = self.read_text_frame()?;
            let raw: Value = serde_json::from_str(&text)?;
            if raw.get("push").is_some() {
                let push: PushFrame = serde_json::from_value(raw)?;
                self.pending_pushes.push_back(push);
                continue;
            }
            if raw.get("error").is_some() {
                let failure: Failure = serde_json::from_value(raw)?;
                return Err(ClientError::Rpc(failure.error.message));
            }
            if raw.get("result").is_some() {
                let answer: Answer = serde_json::from_value(raw)?;
                if answer.id == id {
                    return Ok(answer.result);
                }
                return Err(ClientError::UnexpectedFrame(text));
            }
            return Err(ClientError::UnexpectedFrame(text));
        }
    }

    /// Calls a typed [`Method`] over the WebSocket.
    ///
    /// # Errors
    ///
    /// Returns any transport, auth, RPC, or JSON decoding error.
    pub fn call<M>(&mut self, params: &M::Params) -> Result<M::Output, ClientError>
    where
        M: Method,
        M::Params: Serialize,
        M::Output: DeserializeOwned,
    {
        let value = serde_json::to_value(params)?;
        let result = self.call_raw(M::NAME, &value)?;
        Ok(serde_json::from_value(result)?)
    }

    /// Calls a parameterless typed [`Method`] over the WebSocket.
    ///
    /// # Errors
    ///
    /// Returns any transport, auth, RPC, or JSON decoding error.
    pub fn call_empty<M>(&mut self) -> Result<M::Output, ClientError>
    where
        M: Method,
        M::Output: DeserializeOwned,
    {
        let result = self.call_raw(M::NAME, &json!({}))?;
        Ok(serde_json::from_value(result)?)
    }

    /// Waits for the next [`PushFrame`] from the server (draining buffered pushes first).
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Unauthorized`] on close code 4401, [`ClientError::Closed`] when the
    /// server closes the socket, or a transport/decoding error.
    pub fn next_push(&mut self) -> Result<PushFrame, ClientError> {
        if let Some(push) = self.pending_pushes.pop_front() {
            return Ok(push);
        }
        set_read_timeout(&self.socket, Some(DEFAULT_IO_TIMEOUT))?;
        let text = self.read_text_frame()?;
        serde_json::from_str::<PushFrame>(&text).map_err(ClientError::from)
    }

    /// Waits up to `timeout` for the next [`PushFrame`], returning `Ok(None)` if the timeout
    /// elapses without a frame arriving.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Unauthorized`] on close code 4401, [`ClientError::Closed`] when the
    /// server closes the socket, or a transport/decoding error.
    pub fn try_next_push(&mut self, timeout: Duration) -> Result<Option<PushFrame>, ClientError> {
        if let Some(push) = self.pending_pushes.pop_front() {
            return Ok(Some(push));
        }
        set_read_timeout(&self.socket, Some(timeout))?;
        match self.read_text_frame() {
            Ok(text) => Ok(Some(serde_json::from_str::<PushFrame>(&text)?)),
            Err(ClientError::WebSocket(err)) if is_timeout(&err) => Ok(None),
            Err(other) => Err(other),
        }
    }

    /// Closes the WebSocket cleanly.
    ///
    /// # Errors
    ///
    /// Returns a transport error if sending the close frame fails.
    pub fn close(mut self) -> Result<(), ClientError> {
        self.socket.close(None)?;
        Ok(())
    }

    fn read_text_frame(&mut self) -> Result<String, ClientError> {
        loop {
            match self.socket.read() {
                Ok(Message::Text(text)) => return Ok(text.as_str().to_string()),
                Ok(Message::Close(frame)) => {
                    let code = frame.as_ref().map(|f| u16::from(f.code));
                    if code == Some(UNAUTHORIZED_CLOSE_CODE) {
                        let reason = frame
                            .map(|f| f.reason.to_string())
                            .filter(|r| !r.is_empty())
                            .unwrap_or_else(|| "Unauthorized".to_string());
                        return Err(ClientError::Unauthorized(reason));
                    }
                    return Err(ClientError::Closed(code));
                }
                Ok(Message::Ping(payload)) => {
                    let _ = self.socket.send(Message::Pong(payload));
                }
                Ok(_) => {}
                Err(err) => return Err(ClientError::from(err)),
            }
        }
    }
}

fn build_ws_url(base_url: &str, token: &str) -> Result<Url, ClientError> {
    let mut url =
        Url::parse(base_url).map_err(|e| ClientError::InvalidUrl(format!("{base_url}: {e}")))?;
    let ws_scheme = match url.scheme() {
        "http" | "ws" => "ws",
        "https" | "wss" => "wss",
        other => {
            return Err(ClientError::InvalidUrl(format!(
                "unsupported scheme `{other}`"
            )));
        }
    };
    url.set_scheme(ws_scheme)
        .map_err(|()| ClientError::InvalidUrl(base_url.to_string()))?;
    let path = url.path().trim_end_matches('/');
    if !path.ends_with("/ws") {
        url.set_path(&format!("{path}/ws"));
    }
    url.query_pairs_mut().append_pair("token", token);
    Ok(url)
}

fn tcp_stream(socket: &WebSocket<MaybeTlsStream<TcpStream>>) -> Option<&TcpStream> {
    match socket.get_ref() {
        MaybeTlsStream::Plain(stream) => Some(stream),
        MaybeTlsStream::Rustls(stream) => Some(stream.get_ref()),
        _ => None,
    }
}

fn set_timeouts(
    socket: &WebSocket<MaybeTlsStream<TcpStream>>,
    timeout: Option<Duration>,
) -> Result<(), ClientError> {
    if let Some(stream) = tcp_stream(socket) {
        stream.set_read_timeout(timeout)?;
        stream.set_write_timeout(timeout)?;
    }
    Ok(())
}

fn set_read_timeout(
    socket: &WebSocket<MaybeTlsStream<TcpStream>>,
    timeout: Option<Duration>,
) -> Result<(), ClientError> {
    if let Some(stream) = tcp_stream(socket) {
        stream.set_read_timeout(timeout)?;
    }
    Ok(())
}

fn is_timeout(error: &tungstenite::Error) -> bool {
    matches!(
        error,
        tungstenite::Error::Io(io)
            if matches!(io.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
    )
}
