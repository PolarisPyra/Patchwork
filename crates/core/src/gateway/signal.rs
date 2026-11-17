//! Signal, through a `signal-cli daemon --http` running beside Patchwork: its event stream for
//! messages, and its JSON-RPC endpoint to answer. Signal has no buttons or edits.

// Rust guideline compliant 2026-10-09

use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};

use super::net::{client, failed, json, mime, stream_client};
use super::platform::Platform;
use super::types::{Button, Connector, Incoming, Listener};
use crate::Error;
use crate::system::fs::home_dir;

/// How long the event stream stays open before it is opened again.
const STREAM_FOR: Duration = Duration::from_secs(15 * 60);

/// The prefix of a group chat's id, which is sent to by group id instead of by number.
const GROUP: &str = "group:";

pub(crate) struct Signal {
    /// The daemon's address, without a trailing slash.
    server: String,
    /// The phone number Patchwork sends from.
    account: String,
    http: ureq::Agent,
    stream: ureq::Agent,
    /// Numbers each request.
    calls: AtomicU64,
}

impl Signal {
    pub(crate) fn new(server: String, account: String) -> Self {
        Self {
            server,
            account,
            http: client(),
            stream: stream_client(STREAM_FOR),
            calls: AtomicU64::new(0),
        }
    }

    /// Calls the daemon's JSON-RPC method `method`.
    fn call(&self, method: &str, mut params: Value) -> Result<Value, Error> {
        params["account"] = json!(self.account);
        let body = json!({
            "jsonrpc": "2.0",
            "id": self.calls.fetch_add(1, Ordering::Relaxed),
            "method": method,
            "params": params,
        });
        let response = self
            .http
            .post(&format!("{}/api/v1/rpc", self.server))
            .send_json(&body)
            .map_err(|e| failed("signal-cli could not be reached", e, ""))?;
        let body = json("signal-cli", response)?;
        if let Some(message) = body["error"]["message"].as_str() {
            return Err(format!("signal-cli refused the request: {message}").into());
        }
        Ok(body["result"].clone())
    }

    /// Who a message to `chat` goes to, as the daemon's parameters.
    fn to(chat: &str) -> Value {
        match chat.strip_prefix(GROUP) {
            Some(group) => json!({"groupId": group}),
            None => json!({"recipient": [chat]}),
        }
    }
}

impl Connector for Signal {
    fn listen(&self, to: &Listener<'_>) -> Result<(), Error> {
        let url = format!(
            "{}/api/v1/events?account={}",
            self.server,
            super::net::encode(&self.account)
        );
        let response = self
            .stream
            .get(&url)
            .call()
            .map_err(|e| failed("signal-cli could not be reached", e, ""))?;
        if !response.status().is_success() {
            return Err(format!("signal-cli refused the connection: {}", response.status()).into());
        }
        (to.connected)(self.account.clone());
        let reader = BufReader::new(response.into_body().into_reader());
        for line in reader.lines() {
            // The stream ending, or timing out, only means it is opened again.
            let Ok(line) = line else {
                return Ok(());
            };
            if to.stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let Ok(event) = serde_json::from_str::<Value>(data.trim()) else {
                continue;
            };
            if let Some(message) = incoming(&event) {
                (to.inbox)(message);
            }
        }
        Ok(())
    }

    fn send(&self, chat: &str, text: &str, _buttons: &[Button]) -> Result<Option<String>, Error> {
        let mut params = Self::to(chat);
        params["message"] = json!(text);
        self.call("send", params)?;
        Ok(None)
    }

    fn send_file(&self, chat: &str, name: &str, bytes: &[u8], caption: &str) -> Result<(), Error> {
        let mut params = Self::to(chat);
        params["message"] = json!(caption);
        params["attachments"] = json!([format!(
            "data:{};filename={name};base64,{}",
            if mime(name) == "image/png" {
                "image/png"
            } else {
                "text/plain"
            },
            STANDARD.encode(bytes)
        )]);
        self.call("send", params).map(drop)
    }

    fn edit(&self, _chat: &str, _id: &str, _text: &str) -> Result<(), Error> {
        Err("Signal messages cannot be edited".into())
    }

    fn typing(&self, chat: &str) {
        let _ = self.call("sendTyping", Self::to(chat));
    }
}

/// `id` when it names a file in the attachments folder, never a path out of it.
fn file_name(id: &str) -> Option<&str> {
    let mut parts = std::path::Path::new(id).components();
    let plain = matches!(parts.next(), Some(std::path::Component::Normal(_)));
    (plain && parts.next().is_none()).then_some(id)
}

/// The message in a daemon event, with the pictures sent with it.
fn incoming(event: &Value) -> Option<Incoming> {
    let envelope = &event["envelope"];
    let data = envelope.get("dataMessage")?;
    let user = envelope["sourceNumber"]
        .as_str()
        .or_else(|| envelope["sourceUuid"].as_str())
        .or_else(|| envelope["source"].as_str())?;
    let chat = match data["groupInfo"]["groupId"].as_str() {
        Some(group) => format!("{GROUP}{group}"),
        None => user.to_string(),
    };
    // signal-cli saves attachments in its data folder under their ids.
    let folder = home_dir().map(|h| h.join(".local/share/signal-cli/attachments"));
    let files = data["attachments"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| {
            a["contentType"]
                .as_str()
                .is_some_and(|t| t.starts_with("image/"))
        })
        .filter_map(|a| std::fs::read(folder.as_ref()?.join(file_name(a["id"].as_str()?)?)).ok())
        .collect::<Vec<_>>();
    let text = data["message"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .to_string();
    if text.is_empty() && files.is_empty() {
        return None;
    }
    Some(Incoming {
        platform: Platform::Signal,
        // Signal names a message by when its sender sent it.
        id: envelope["timestamp"]
            .as_u64()
            .map(|at| format!("{user}:{at}")),
        chat,
        user: user.to_string(),
        name: envelope["sourceName"].as_str().unwrap_or(user).to_string(),
        text,
        files,
    })
}
