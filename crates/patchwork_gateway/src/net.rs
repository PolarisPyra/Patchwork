//! The HTTP requests and websockets the chat app connectors talk over.

// Rust guideline compliant 2026-10-09

use std::fmt::Write as _;
use std::io::ErrorKind;
use std::net::TcpStream;
use std::time::Duration;

use serde_json::Value;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use patchwork_core::Error;

pub(crate) use patchwork_core::gateway::{is_picture, mime};

/// How long a request may take, which long polls stay under.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(40);

/// How long a websocket read waits before the connector checks whether to stop or heartbeat.
const SOCKET_WAIT: Duration = Duration::from_secs(1);

/// The largest file Patchwork downloads from a chat app, in bytes.
const MAX_FILE: u64 = 25 * 1024 * 1024;

/// A websocket to a chat app.
pub(crate) type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

/// An HTTP client that returns error statuses as responses, so their bodies can be read.
pub(crate) fn client() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .http_status_as_error(false)
        .build()
        .into()
}

/// An HTTP client for a stream that stays open for `wait` between events.
pub(crate) fn stream_client(wait: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(None)
        .timeout_recv_body(Some(wait))
        .http_status_as_error(false)
        .build()
        .into()
}

/// An error about reaching `what`, with `secret` taken out of it in case the message quotes a
/// URL it is part of.
pub(crate) fn failed(what: &str, error: impl std::fmt::Display, secret: &str) -> Error {
    let text = error.to_string();
    let text = if secret.is_empty() {
        text
    } else {
        text.replace(secret, "…")
    };
    format!("{what}: {text}").into()
}

/// The JSON body of `response`, or an error naming its status when it is not a success.
pub(crate) fn json(
    what: &str,
    mut response: ureq::http::Response<ureq::Body>,
) -> Result<Value, Error> {
    let status = response.status();
    let body: Value = response.body_mut().read_json().unwrap_or(Value::Null);
    if status.is_success() {
        return Ok(body);
    }
    let reason = ["message", "error", "description"]
        .iter()
        .find_map(|key| body[*key].as_str())
        .map_or_else(|| status.to_string(), str::to_string);
    Err(format!("{what} refused the request: {reason}").into())
}

/// Sends a request with `auth` as its `Authorization` header: a PUT or a POST of `body` when one
/// is given, or else a GET.
pub(crate) fn authorized(
    http: &ureq::Agent,
    method: &str,
    url: &str,
    auth: &str,
    body: Option<&Value>,
) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
    match (method, body) {
        ("PUT", Some(body)) => http.put(url).header("Authorization", auth).send_json(body),
        (_, Some(body)) => http.post(url).header("Authorization", auth).send_json(body),
        _ => http.get(url).header("Authorization", auth).call(),
    }
}

/// Downloads a file, with `auth` as its `Authorization` header when it is given.
///
/// # Errors
///
/// Returns an error when the file cannot be downloaded or is larger than Patchwork takes.
pub(crate) fn download(
    http: &ureq::Agent,
    url: &str,
    auth: Option<&str>,
) -> Result<Vec<u8>, Error> {
    let mut request = http.get(url);
    if let Some(auth) = auth {
        request = request.header("Authorization", auth);
    }
    let mut response = request
        .call()
        .map_err(|e| failed("Could not download a file", e, auth.unwrap_or_default()))?;
    if !response.status().is_success() {
        return Err(format!("Could not download a file: {}", response.status()).into());
    }
    response
        .body_mut()
        .with_config()
        .limit(MAX_FILE)
        .read_to_vec()
        .map_err(|e| e.to_string().into())
}

/// Opens a websocket to `url`, with reads that give up after `SOCKET_WAIT`.
///
/// # Errors
///
/// Returns an error when the server cannot be reached or refuses the connection.
pub(crate) fn connect(what: &str, url: &str) -> Result<Socket, Error> {
    let (socket, _) = tungstenite::connect(url).map_err(|e| failed(what, e, ""))?;
    let stream = match socket.get_ref() {
        MaybeTlsStream::Plain(stream) => Some(stream),
        MaybeTlsStream::Rustls(stream) => Some(stream.get_ref()),
        _ => None,
    };
    if let Some(stream) = stream {
        stream
            .set_read_timeout(Some(SOCKET_WAIT))
            .map_err(|e| failed(what, e, ""))?;
    }
    Ok(socket)
}

/// The next text message on `socket`, or none when nothing arrived in time.
///
/// # Errors
///
/// Returns an error when the connection closes or fails.
pub(crate) fn read(what: &str, socket: &mut Socket) -> Result<Option<String>, Error> {
    match socket.read() {
        Ok(Message::Text(text)) => Ok(Some(text.as_str().to_string())),
        Ok(Message::Close(frame)) => Err(match frame {
            Some(frame) if !frame.reason.is_empty() => {
                format!("{what} closed the connection: {}", frame.reason).into()
            }
            Some(frame) => format!("{what} closed the connection ({})", frame.code).into(),
            None => format!("{what} closed the connection").into(),
        }),
        Ok(_) => Ok(None),
        Err(tungstenite::Error::Io(e))
            if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
        {
            Ok(None)
        }
        Err(e) => Err(failed(what, e, "")),
    }
}

/// Sends `value` as a text message on `socket`.
///
/// # Errors
///
/// Returns an error when the connection fails.
pub(crate) fn write(what: &str, socket: &mut Socket, value: &Value) -> Result<(), Error> {
    socket
        .send(Message::text(value.to_string()))
        .map_err(|e| failed(what, e, ""))
}

/// A `multipart/form-data` body of the text `fields` and the file `name` holding `bytes` under
/// `field`, and the content type to send it with.
pub(crate) fn multipart(
    fields: &[(&str, &str)],
    field: &str,
    name: &str,
    bytes: &[u8],
) -> (String, Vec<u8>) {
    let boundary = format!("patchwork-{}", uuid::Uuid::new_v4().simple());
    // A quote or line break in the name would end its header and let the rest of it be read as
    // more of the request.
    let name: String = name
        .chars()
        .map(|c| {
            if matches!(c, '"' | '\\' | '\r' | '\n') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let mut body = Vec::with_capacity(bytes.len() + 512);
    for (key, value) in fields {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{key}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{name}\"\r\nContent-Type: {}\r\n\r\n",
            mime(bytes)
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

/// Splits `text` into pieces of at most `limit` characters, at line breaks where it can.
pub(crate) fn chunks(text: &str, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text.trim();
    while !rest.is_empty() {
        if rest.chars().count() <= limit {
            out.push(rest.to_string());
            break;
        }
        let end = rest
            .char_indices()
            .nth(limit)
            .map_or(rest.len(), |(at, _)| at);
        let head = &rest[..end];
        let cut = head
            .rfind('\n')
            .filter(|&at| at > end / 2)
            .or_else(|| head.rfind(' ').filter(|&at| at > end / 2))
            .unwrap_or(end);
        out.push(rest[..cut].trim_end().to_string());
        rest = rest[cut..].trim_start();
    }
    out
}

/// `text` with everything but unreserved URL characters percent-encoded, for a path segment.
pub(crate) fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}
