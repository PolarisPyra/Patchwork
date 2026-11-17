//! Matrix, through the client-server API: a long `/sync` for messages and invites. Rooms with
//! end-to-end encryption are not read.

// Rust guideline compliant 2026-10-09

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use serde_json::{Value, json};

use super::net::{client, download, encode, failed, json, mime};
use super::platform::Platform;
use super::types::{Button, Connector, Incoming, Listener};
use crate::Error;
use crate::system::now_ms;

/// How long each sync waits for an event, in milliseconds; under the request timeout.
const SYNC_WAIT: u64 = 10_000;

/// How long the typing notice lasts unless a message ends it, in milliseconds.
const TYPING_FOR: u64 = 30_000;

/// The first sync's filter: no history, only where each room stands.
const FIRST_FILTER: &str = r#"{"room":{"timeline":{"limit":1}}}"#;

pub(crate) struct Matrix {
    /// The homeserver, without a trailing slash.
    server: String,
    token: String,
    http: ureq::Agent,
    /// The bot's user id, once connected.
    me: Mutex<String>,
    /// Makes each sent event's transaction id unique.
    sent: AtomicU64,
}

impl Matrix {
    pub(crate) fn new(server: String, token: String) -> Self {
        Self {
            server,
            token,
            http: client(),
            me: Mutex::default(),
            sent: AtomicU64::new(0),
        }
    }

    fn auth(&self) -> String {
        format!("Bearer {}", self.token)
    }

    /// Calls the client-server API at `path` with `method`.
    fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, Error> {
        let url = format!("{}{path}", self.server);
        let auth = self.auth();
        let response = match (method, body) {
            ("PUT", Some(body)) => self
                .http
                .put(&url)
                .header("Authorization", &auth)
                .send_json(body),
            (_, Some(body)) => self
                .http
                .post(&url)
                .header("Authorization", &auth)
                .send_json(body),
            _ => self.http.get(&url).header("Authorization", &auth).call(),
        };
        let response =
            response.map_err(|e| failed("The homeserver could not be reached", e, &self.token))?;
        json("The homeserver", response)
    }

    fn sync(&self, since: Option<&str>) -> Result<Value, Error> {
        let path = match since {
            Some(since) => format!(
                "/_matrix/client/v3/sync?timeout={SYNC_WAIT}&since={}",
                encode(since)
            ),
            None => format!(
                "/_matrix/client/v3/sync?timeout=0&filter={}",
                encode(FIRST_FILTER)
            ),
        };
        self.call("GET", &path, None)
    }

    /// Joins the rooms the bot was invited to.
    fn join_invites(&self, sync: &Value) {
        for room in sync["rooms"]["invite"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(id, _)| id)
        {
            let path = format!("/_matrix/client/v3/rooms/{}/join", encode(room));
            if let Err(error) = self.call("POST", &path, Some(&json!({}))) {
                log::warn!(error:% = error; "could not join a Matrix room: {{error}}");
            }
        }
    }

    /// The messages others sent in the rooms the bot is in.
    fn messages(&self, sync: &Value) -> Vec<Incoming> {
        let me = self
            .me
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let mut out = Vec::new();
        for (room, data) in sync["rooms"]["join"].as_object().into_iter().flatten() {
            for event in data["timeline"]["events"].as_array().into_iter().flatten() {
                let content = &event["content"];
                let sender = event["sender"].as_str().unwrap_or_default();
                let edit = content["m.relates_to"]["rel_type"].as_str() == Some("m.replace");
                if event["type"].as_str() != Some("m.room.message") || sender == me || edit {
                    continue;
                }
                let (text, files) = match content["msgtype"].as_str() {
                    Some("m.text" | "m.notice") => {
                        (content["body"].as_str().unwrap_or_default(), Vec::new())
                    }
                    Some("m.image") => {
                        let file = content["url"].as_str().and_then(|url| self.media(url));
                        ("", file.into_iter().collect())
                    }
                    _ => continue,
                };
                out.push(Incoming {
                    platform: Platform::Matrix,
                    id: event["event_id"].as_str().map(str::to_string),
                    chat: room.clone(),
                    user: sender.to_string(),
                    name: sender.to_string(),
                    text: text.trim().to_string(),
                    files,
                });
            }
        }
        out
    }

    /// Downloads the file at an `mxc://` address.
    fn media(&self, mxc: &str) -> Option<Vec<u8>> {
        let (server, id) = mxc.strip_prefix("mxc://")?.split_once('/')?;
        let url = format!(
            "{}/_matrix/client/v1/media/download/{}/{}",
            self.server,
            encode(server),
            encode(id)
        );
        download(&self.http, &url, Some(&self.auth())).ok()
    }

    /// Sends a message event to `room` and returns its id.
    fn put_message(&self, room: &str, content: &Value) -> Result<Option<String>, Error> {
        let txn = format!(
            "pw{}-{}",
            now_ms(),
            self.sent.fetch_add(1, Ordering::Relaxed)
        );
        let path = format!(
            "/_matrix/client/v3/rooms/{}/send/m.room.message/{txn}",
            encode(room)
        );
        let sent = self.call("PUT", &path, Some(content))?;
        Ok(sent["event_id"].as_str().map(str::to_string))
    }
}

impl Connector for Matrix {
    fn listen(&self, to: &Listener<'_>) -> Result<(), Error> {
        let me = self.call("GET", "/_matrix/client/v3/account/whoami", None)?;
        let me = me["user_id"]
            .as_str()
            .ok_or("The homeserver did not say who the token belongs to")?
            .to_string();
        me.clone_into(&mut self.me.lock().unwrap_or_else(PoisonError::into_inner));
        // The first sync only marks where Patchwork starts: older messages are not answered.
        let first = self.sync(None)?;
        self.join_invites(&first);
        let mut since = first["next_batch"].as_str().unwrap_or_default().to_string();
        (to.connected)(me);
        while !to.stop.load(Ordering::Relaxed) {
            let sync = self.sync(Some(&since))?;
            if to.stop.load(Ordering::Relaxed) {
                break;
            }
            self.join_invites(&sync);
            for message in self.messages(&sync) {
                (to.inbox)(message);
            }
            if let Some(next) = sync["next_batch"].as_str() {
                since = next.to_string();
            }
        }
        Ok(())
    }

    fn send(&self, chat: &str, text: &str, _buttons: &[Button]) -> Result<Option<String>, Error> {
        self.put_message(chat, &json!({"msgtype": "m.text", "body": text}))
    }

    fn send_file(&self, chat: &str, name: &str, bytes: &[u8], caption: &str) -> Result<(), Error> {
        let url = format!(
            "{}/_matrix/media/v3/upload?filename={}",
            self.server,
            encode(name)
        );
        let response = self
            .http
            .post(&url)
            .header("Authorization", &self.auth())
            .header("Content-Type", mime(name))
            .send(bytes)
            .map_err(|e| failed("The homeserver could not be reached", e, &self.token))?;
        let uploaded = json("The homeserver", response)?;
        let uri = uploaded["content_uri"]
            .as_str()
            .ok_or("The homeserver did not say where it kept the file")?;
        if !caption.is_empty() {
            self.put_message(chat, &json!({"msgtype": "m.text", "body": caption}))?;
        }
        let picture = mime(name) == "image/png";
        self.put_message(
            chat,
            &json!({
                "msgtype": if picture { "m.image" } else { "m.file" },
                "body": name,
                "filename": name,
                "url": uri,
                "info": {"mimetype": if picture { "image/png" } else { "text/plain" }, "size": bytes.len()},
            }),
        )
        .map(drop)
    }

    fn edit(&self, chat: &str, id: &str, text: &str) -> Result<(), Error> {
        let content = json!({
            "msgtype": "m.text",
            "body": format!("* {text}"),
            "m.new_content": {"msgtype": "m.text", "body": text},
            "m.relates_to": {"rel_type": "m.replace", "event_id": id},
        });
        self.put_message(chat, &content).map(drop)
    }

    fn typing(&self, chat: &str) {
        let me = self
            .me
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let path = format!(
            "/_matrix/client/v3/rooms/{}/typing/{}",
            encode(chat),
            encode(&me)
        );
        let _ = self.call(
            "PUT",
            &path,
            Some(&json!({"typing": true, "timeout": TYPING_FOR})),
        );
    }
}
