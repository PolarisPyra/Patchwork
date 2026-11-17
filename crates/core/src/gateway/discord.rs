//! Discord: the gateway websocket for messages and button presses, and the REST API to answer.

// Rust guideline compliant 2026-10-09

use std::sync::atomic::Ordering;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::net::{Socket, client, connect, download, failed, json, read, write};
use super::platform::Platform;
use super::types::{Button, Connector, Incoming, Listener};
use crate::Error;

const API: &str = "https://discord.com/api/v10";

/// The user agent Discord asks bots to send.
const USER_AGENT: &str = concat!("DiscordBot (patchwork, ", env!("CARGO_PKG_VERSION"), ")");

/// Server messages and direct messages. Without the privileged message content intent, a server
/// message only has text when it mentions the bot.
const INTENTS: u64 = (1 << 9) | (1 << 12);

/// The most buttons in one row.
const ROW: usize = 5;

/// The interaction answer that replaces the message the button was on.
const UPDATE_MESSAGE: u64 = 7;

pub(crate) struct Discord {
    token: String,
    http: ureq::Agent,
    /// The bot's user id, once connected.
    me: Mutex<String>,
}

impl Discord {
    pub(crate) fn new(token: String) -> Self {
        Self {
            token,
            http: client(),
            me: Mutex::default(),
        }
    }

    fn auth(&self) -> String {
        format!("Bot {}", self.token)
    }

    /// Calls the REST API and returns its answer.
    fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, Error> {
        let url = format!("{API}{path}");
        let auth = self.auth();
        let response = match (method, body) {
            ("GET", _) => self
                .http
                .get(&url)
                .header("Authorization", &auth)
                .header("User-Agent", USER_AGENT)
                .call(),
            ("PATCH", Some(body)) => self
                .http
                .patch(&url)
                .header("Authorization", &auth)
                .header("User-Agent", USER_AGENT)
                .send_json(body),
            (_, Some(body)) => self
                .http
                .post(&url)
                .header("Authorization", &auth)
                .header("User-Agent", USER_AGENT)
                .send_json(body),
            (_, None) => self
                .http
                .post(&url)
                .header("Authorization", &auth)
                .header("User-Agent", USER_AGENT)
                .send_empty(),
        };
        let response =
            response.map_err(|e| failed("Discord could not be reached", e, &self.token))?;
        json("Discord", response)
    }

    /// Handles one gateway payload, returning an error when Discord asks for a new connection.
    fn dispatch(&self, payload: &Value, to: &Listener<'_>) -> Result<(), Error> {
        match payload["op"].as_u64() {
            Some(7) => return Err("Discord asked for a new connection".into()),
            Some(9) => return Err("Discord ended the session".into()),
            Some(0) => {}
            _ => return Ok(()),
        }
        let data = &payload["d"];
        match payload["t"].as_str() {
            Some("READY") => {
                let user = &data["user"];
                *self.me.lock().unwrap_or_else(PoisonError::into_inner) =
                    user["id"].as_str().unwrap_or_default().to_string();
                (to.connected)(user["username"].as_str().unwrap_or("bot").to_string());
            }
            Some("MESSAGE_CREATE") => {
                if let Some(message) = self.message(data) {
                    (to.inbox)(message);
                }
            }
            Some("INTERACTION_CREATE") => {
                if let Some(message) = self.press(data) {
                    (to.inbox)(message);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn message(&self, data: &Value) -> Option<Incoming> {
        let author = &data["author"];
        if author["bot"].as_bool() == Some(true) {
            return None;
        }
        let me = self
            .me
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let mut text = data["content"].as_str().unwrap_or_default().to_string();
        // In a server, the bot answers only when it is mentioned.
        if data.get("guild_id").is_some_and(|g| !g.is_null()) {
            let mentioned = data["mentions"]
                .as_array()
                .is_some_and(|m| m.iter().any(|u| u["id"].as_str() == Some(&me)));
            if !mentioned {
                return None;
            }
            text = text
                .replace(&format!("<@{me}>"), "")
                .replace(&format!("<@!{me}>"), "");
        }
        let files = data["attachments"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|a| {
                a["content_type"]
                    .as_str()
                    .is_some_and(|t| t.starts_with("image/"))
            })
            .filter_map(|a| download(&self.http, a["url"].as_str()?, None).ok())
            .collect();
        Some(Incoming {
            platform: Platform::Discord,
            chat: data["channel_id"].as_str()?.to_string(),
            user: author["id"].as_str()?.to_string(),
            name: author["username"].as_str().unwrap_or_default().to_string(),
            text: text.trim().to_string(),
            files,
        })
    }

    /// A button press, answered by taking the buttons off its message.
    fn press(&self, data: &Value) -> Option<Incoming> {
        let reply = data["data"]["custom_id"].as_str()?;
        let path = format!(
            "/interactions/{}/{}/callback",
            data["id"].as_str()?,
            data["token"].as_str()?
        );
        let answer = json!({"type": UPDATE_MESSAGE, "data": {"components": []}});
        let _ = self.call("POST", &path, Some(&answer));
        let user = data["member"]["user"]
            .as_object()
            .or_else(|| data["user"].as_object())?;
        Some(Incoming {
            platform: Platform::Discord,
            chat: data["channel_id"].as_str()?.to_string(),
            user: user.get("id")?.as_str()?.to_string(),
            name: user
                .get("username")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            text: reply.to_string(),
            files: Vec::new(),
        })
    }
}

impl Connector for Discord {
    fn listen(&self, to: &Listener<'_>) -> Result<(), Error> {
        let gateway = self.call("GET", "/gateway/bot", None)?;
        let url = gateway["url"]
            .as_str()
            .ok_or("Discord did not say where to connect")?;
        let mut socket = connect("Discord", &format!("{url}/?v=10&encoding=json"))?;
        let beat = hello(&mut socket, to)?;
        write(
            "Discord",
            &mut socket,
            &json!({"op": 2, "d": {
                "token": self.token,
                "intents": INTENTS,
                "properties": {"os": std::env::consts::OS, "browser": "patchwork", "device": "patchwork"},
            }}),
        )?;
        let mut seq = Value::Null;
        let mut next = Instant::now() + beat;
        while !to.stop.load(Ordering::Relaxed) {
            if Instant::now() >= next {
                write("Discord", &mut socket, &json!({"op": 1, "d": seq}))?;
                next = Instant::now() + beat;
            }
            let Some(text) = read("Discord", &mut socket)? else {
                continue;
            };
            let Ok(payload) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            if !payload["s"].is_null() {
                seq = payload["s"].clone();
            }
            if payload["op"].as_u64() == Some(1) {
                next = Instant::now();
            }
            self.dispatch(&payload, to)?;
        }
        let _ = socket.close(None);
        Ok(())
    }

    fn send(&self, chat: &str, text: &str, buttons: &[Button]) -> Result<Option<String>, Error> {
        let mut body = json!({"content": text});
        if !buttons.is_empty() {
            let rows: Vec<Value> = buttons
                .chunks(ROW)
                .map(|row| {
                    let row: Vec<Value> = row
                        .iter()
                        .map(|b| json!({"type": 2, "style": 2, "label": b.label, "custom_id": b.reply}))
                        .collect();
                    json!({"type": 1, "components": row})
                })
                .collect();
            body["components"] = Value::Array(rows);
        }
        let sent = self.call("POST", &format!("/channels/{chat}/messages"), Some(&body))?;
        Ok(sent["id"].as_str().map(str::to_string))
    }

    fn edit(&self, chat: &str, id: &str, text: &str) -> Result<(), Error> {
        let path = format!("/channels/{chat}/messages/{id}");
        self.call("PATCH", &path, Some(&json!({"content": text})))
            .map(drop)
    }

    fn typing(&self, chat: &str) {
        let _ = self.call("POST", &format!("/channels/{chat}/typing"), None);
    }
}

/// Waits for Discord's hello and returns how often to heartbeat.
fn hello(socket: &mut Socket, to: &Listener<'_>) -> Result<Duration, Error> {
    while !to.stop.load(Ordering::Relaxed) {
        let Some(text) = read("Discord", socket)? else {
            continue;
        };
        let payload: Value = serde_json::from_str(&text).unwrap_or_default();
        if payload["op"].as_u64() == Some(10) {
            let ms = payload["d"]["heartbeat_interval"]
                .as_u64()
                .unwrap_or(41_250);
            return Ok(Duration::from_millis(ms));
        }
    }
    Err("Stopped".into())
}
