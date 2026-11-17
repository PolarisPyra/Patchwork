//! QQ, through the official bot API: its websocket gateway for messages, and the REST API to
//! answer. A reply quotes the message it answers, which QQ requires within a few minutes of it in
//! groups. QQ has no edits or buttons here.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::net::{Socket, client, connect, download, failed, json, read, write};
use super::platform::Platform;
use super::types::{Button, Connector, Incoming, Listener};
use crate::Error;
use crate::system::now_ms;

/// Where access tokens come from.
const TOKEN_URL: &str = "https://bots.qq.com/app/getAppAccessToken";

const API: &str = "https://api.sgroup.qq.com";

/// Direct and group messages, guild messages that mention the bot, and guild direct messages.
const INTENTS: u64 = (1 << 25) | (1 << 30) | (1 << 12);

/// How long before a token expires that Patchwork fetches another.
const MARGIN: Duration = Duration::from_secs(60);

/// The range reply numbers are drawn from.
const SEQ_RANGE: u64 = 1_000_000_000;

/// How often to heartbeat when QQ's hello does not say.
const DEFAULT_BEAT: Duration = Duration::from_secs(41);

pub(crate) struct Qq {
    app: String,
    secret: String,
    http: ureq::Agent,
    /// The access token, and when Patchwork stops using it.
    token: Mutex<Option<(String, Instant)>>,
    /// The last message each chat sent, which a reply quotes.
    last: Mutex<HashMap<String, String>>,
    /// Numbers the replies, which QQ needs to tell apart several answers to one message.
    seq: AtomicU64,
}

impl Qq {
    pub(crate) fn new(app: String, secret: String) -> Self {
        Self {
            app,
            secret,
            http: client(),
            token: Mutex::new(None),
            last: Mutex::new(HashMap::new()),
            // Seeded from the clock, so a restart does not reuse numbers QQ has seen.
            seq: AtomicU64::new(now_ms() % SEQ_RANGE),
        }
    }

    /// The `Authorization` header, with a token fetched again before it expires.
    fn auth(&self) -> Result<String, Error> {
        let mut kept = self.token.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((token, until)) = kept.as_ref()
            && Instant::now() < *until
        {
            return Ok(format!("QQBot {token}"));
        }
        let response = self
            .http
            .post(TOKEN_URL)
            .send_json(json!({"appId": self.app, "clientSecret": self.secret}))
            .map_err(|e| failed("QQ could not be reached", e, ""))?;
        let body = json("QQ", response)?;
        let token = body["access_token"]
            .as_str()
            .ok_or("QQ refused the app ID or secret")?
            .to_string();
        // QQ sends the lifetime as a string.
        let seconds = body["expires_in"]
            .as_u64()
            .or_else(|| body["expires_in"].as_str()?.parse().ok())
            .unwrap_or(7200);
        let until = Instant::now() + Duration::from_secs(seconds).saturating_sub(MARGIN);
        *kept = Some((token.clone(), until));
        Ok(format!("QQBot {token}"))
    }

    fn call(&self, path: &str, body: Option<&Value>) -> Result<Value, Error> {
        let url = format!("{API}{path}");
        let auth = self.auth()?;
        let response = match body {
            Some(body) => self
                .http
                .post(&url)
                .header("Authorization", &auth)
                .send_json(body),
            None => self.http.get(&url).header("Authorization", &auth).call(),
        };
        let response = response.map_err(|e| failed("QQ could not be reached", e, ""))?;
        json("QQ", response)
    }

    /// The pictures attached to a message.
    fn pictures(&self, data: &Value) -> Vec<Vec<u8>> {
        let auth = self.auth().ok();
        data["attachments"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|a| {
                a["content_type"]
                    .as_str()
                    .is_some_and(|t| t.starts_with("image/"))
            })
            .filter_map(|a| a["url"].as_str())
            .filter_map(|url| {
                let url = if url.starts_with("//") {
                    format!("https:{url}")
                } else {
                    url.to_string()
                };
                download(&self.http, &url, auth.as_deref()).ok()
            })
            .collect()
    }

    /// The message in a gateway event. The chat is the kind of conversation and its id, which
    /// says where the answer goes.
    fn incoming(&self, event: &str, data: &Value) -> Option<Incoming> {
        let author = &data["author"];
        let (chat, user) = match event {
            "C2C_MESSAGE_CREATE" => {
                let user = author["user_openid"].as_str()?;
                (format!("c2c:{user}"), user)
            }
            "GROUP_AT_MESSAGE_CREATE" => (
                format!("group:{}", data["group_openid"].as_str()?),
                author["member_openid"].as_str()?,
            ),
            "AT_MESSAGE_CREATE" | "GUILD_AT_MESSAGE_CREATE" => (
                format!("channel:{}", data["channel_id"].as_str()?),
                author["id"].as_str()?,
            ),
            "DIRECT_MESSAGE_CREATE" => (
                format!("dm:{}", data["guild_id"].as_str()?),
                author["id"].as_str()?,
            ),
            _ => return None,
        };
        if author["bot"] == true {
            return None;
        }
        if let Some(id) = data["id"].as_str() {
            self.last
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(chat.clone(), id.to_string());
        }
        let content = data["content"].as_str().unwrap_or_default().trim();
        // A message that mentions the bot starts with the mention.
        let text = match event {
            "AT_MESSAGE_CREATE" | "GUILD_AT_MESSAGE_CREATE" | "GROUP_AT_MESSAGE_CREATE"
                if content.starts_with("<@") =>
            {
                content
                    .split_once('>')
                    .map_or(content, |(_, rest)| rest)
                    .trim()
            }
            _ => content,
        };
        let files = self.pictures(data);
        if text.is_empty() && files.is_empty() {
            return None;
        }
        Some(Incoming {
            platform: Platform::Qq,
            id: data["id"].as_str().map(str::to_string),
            chat,
            user: user.to_string(),
            name: author["username"].as_str().unwrap_or(user).to_string(),
            text: text.to_string(),
            files,
        })
    }
}

impl Connector for Qq {
    fn listen(&self, to: &Listener<'_>) -> Result<(), Error> {
        let gateway = self.call("/gateway", None)?;
        let url = gateway["url"]
            .as_str()
            .ok_or("QQ did not say where to connect")?;
        let mut socket = connect("QQ", url)?;
        let beat = hello(&mut socket, to)?;
        write(
            "QQ",
            &mut socket,
            &json!({"op": 2, "d": {
                "token": self.auth()?,
                "intents": INTENTS,
                "shard": [0, 1],
                "properties": {"$os": std::env::consts::OS, "$browser": "patchwork", "$device": "patchwork"},
            }}),
        )?;
        let mut seq = Value::Null;
        // Beat a little early, so a slow write does not miss QQ's deadline.
        let beat = beat * 4 / 5;
        let mut next = Instant::now() + beat;
        while !to.stop.load(Ordering::Relaxed) {
            if Instant::now() >= next {
                write("QQ", &mut socket, &json!({"op": 1, "d": seq}))?;
                next = Instant::now() + beat;
            }
            let Some(text) = read("QQ", &mut socket)? else {
                continue;
            };
            let Ok(payload) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            if !payload["s"].is_null() {
                seq = payload["s"].clone();
            }
            match payload["op"].as_u64() {
                Some(7) => return Err("QQ asked for a new connection".into()),
                Some(9) => return Err("QQ refused the session".into()),
                Some(0) => {}
                _ => continue,
            }
            let data = &payload["d"];
            match payload["t"].as_str() {
                Some("READY") => {
                    let name = data["user"]["username"].as_str().unwrap_or("bot");
                    (to.connected)(name.to_string());
                }
                Some(event) => {
                    if let Some(message) = self.incoming(event, data) {
                        (to.inbox)(message);
                    }
                }
                None => {}
            }
        }
        let _ = socket.close(None);
        Ok(())
    }

    fn send(&self, chat: &str, text: &str, _buttons: &[Button]) -> Result<Option<String>, Error> {
        let (kind, id) = chat.split_once(':').ok_or("This QQ chat is not known")?;
        let path = match kind {
            "c2c" => format!("/v2/users/{id}/messages"),
            "group" => format!("/v2/groups/{id}/messages"),
            "channel" => format!("/channels/{id}/messages"),
            "dm" => format!("/dms/{id}/messages"),
            _ => return Err("This QQ chat is not known".into()),
        };
        let mut body = json!({"content": text, "msg_type": 0});
        if let Some(quoted) = self
            .last
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(chat)
        {
            body["msg_id"] = json!(quoted);
        }
        if matches!(kind, "c2c" | "group") {
            body["msg_seq"] = json!(self.seq.fetch_add(1, Ordering::Relaxed));
        }
        self.call(&path, Some(&body))?;
        Ok(None)
    }

    fn edit(&self, _chat: &str, _id: &str, _text: &str) -> Result<(), Error> {
        Err("QQ messages cannot be edited".into())
    }
}

/// Waits for QQ's hello and returns how often to heartbeat.
fn hello(socket: &mut Socket, to: &Listener<'_>) -> Result<Duration, Error> {
    while !to.stop.load(Ordering::Relaxed) {
        let Some(text) = read("QQ", socket)? else {
            continue;
        };
        let payload: Value = serde_json::from_str(&text).unwrap_or_default();
        if payload["op"].as_u64() == Some(10) {
            return Ok(payload["d"]["heartbeat_interval"]
                .as_u64()
                .map_or(DEFAULT_BEAT, Duration::from_millis));
        }
    }
    Err("Stopped".into())
}
