//! Slack, through Socket Mode: events and button presses over a websocket the app token opens,
//! and the Web API with the bot token to answer.

// Rust guideline compliant 2026-10-09

use std::sync::atomic::Ordering;
use std::sync::{Mutex, PoisonError};

use serde_json::{Value, json};

use super::net::{Socket, client, connect, download, failed, json, mime, read, write};
use super::platform::Platform;
use super::types::{Button, Connector, Incoming, Listener};
use crate::Error;

const API: &str = "https://slack.com/api";

pub(crate) struct Slack {
    /// The `xoxb-` token the bot acts with.
    bot: String,
    /// The `xapp-` token that opens Socket Mode connections.
    app: String,
    http: ureq::Agent,
    /// The bot's user id, once connected.
    me: Mutex<String>,
}

impl Slack {
    pub(crate) fn new(bot: String, app: String) -> Self {
        Self {
            bot,
            app,
            http: client(),
            me: Mutex::default(),
        }
    }

    /// Calls the Web API method `method` with `token`.
    fn call(&self, method: &str, token: &str, body: &Value) -> Result<Value, Error> {
        let response = self
            .http
            .post(&format!("{API}/{method}"))
            .header("Authorization", &format!("Bearer {token}"))
            .send_json(body)
            .map_err(|e| failed("Slack could not be reached", e, token))?;
        result(response)
    }

    /// Calls the Web API method `method` with the bot token and `fields` as a form, for the
    /// methods that take no JSON.
    fn call_form(&self, method: &str, fields: &[(&str, &str)]) -> Result<Value, Error> {
        let response = self
            .http
            .post(&format!("{API}/{method}"))
            .header("Authorization", &format!("Bearer {}", self.bot))
            .send_form(fields.iter().copied())
            .map_err(|e| failed("Slack could not be reached", e, &self.bot))?;
        result(response)
    }

    /// Handles one Socket Mode envelope, acknowledging it first as Slack asks.
    fn envelope(
        &self,
        socket: &mut Socket,
        envelope: &Value,
        to: &Listener<'_>,
    ) -> Result<bool, Error> {
        if let Some(id) = envelope["envelope_id"].as_str() {
            write("Slack", socket, &json!({"envelope_id": id}))?;
        }
        let payload = &envelope["payload"];
        let message = match envelope["type"].as_str() {
            Some("disconnect") => return Ok(false),
            Some("events_api") => self.event(&payload["event"]),
            Some("interactive") => self.press(payload),
            _ => None,
        };
        if let Some(message) = message {
            (to.inbox)(message);
        }
        Ok(true)
    }

    /// A direct message, or a mention in a channel.
    fn event(&self, event: &Value) -> Option<Incoming> {
        let me = self
            .me
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let kind = event["type"].as_str()?;
        let direct = kind == "message"
            && event["channel_type"].as_str() == Some("im")
            && event["bot_id"].is_null()
            && matches!(event["subtype"].as_str(), None | Some("file_share"));
        if !direct && kind != "app_mention" {
            return None;
        }
        let text = event["text"]
            .as_str()
            .unwrap_or_default()
            .replace(&format!("<@{me}>"), "");
        let auth = format!("Bearer {}", self.bot);
        let files = event["files"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|f| {
                f["mimetype"]
                    .as_str()
                    .is_some_and(|m| m.starts_with("image/"))
            })
            .filter_map(|f| {
                download(&self.http, f["url_private_download"].as_str()?, Some(&auth)).ok()
            })
            .collect();
        let user = event["user"].as_str()?.to_string();
        Some(Incoming {
            platform: Platform::Slack,
            id: event["client_msg_id"]
                .as_str()
                .or_else(|| event["ts"].as_str())
                .map(str::to_string),
            chat: event["channel"].as_str()?.to_string(),
            name: user.clone(),
            user,
            text: text.trim().to_string(),
            files,
        })
    }

    /// A button press, answered by taking the buttons off its message.
    fn press(&self, payload: &Value) -> Option<Incoming> {
        if payload["type"].as_str() != Some("block_actions") {
            return None;
        }
        let reply = payload["actions"][0]["value"].as_str()?;
        let chat = payload["channel"]["id"].as_str()?.to_string();
        let message = &payload["message"];
        let _ = self.call(
            "chat.update",
            &self.bot,
            &json!({"channel": chat, "ts": message["ts"], "text": message["text"], "blocks": []}),
        );
        let user = &payload["user"];
        Some(Incoming {
            platform: Platform::Slack,
            id: payload["actions"][0]["action_ts"]
                .as_str()
                .map(str::to_string),
            chat,
            user: user["id"].as_str()?.to_string(),
            name: user["username"].as_str().unwrap_or_default().to_string(),
            text: reply.to_string(),
            files: Vec::new(),
        })
    }
}

impl Connector for Slack {
    fn listen(&self, to: &Listener<'_>) -> Result<(), Error> {
        let me = self.call("auth.test", &self.bot, &json!({}))?;
        *self.me.lock().unwrap_or_else(PoisonError::into_inner) =
            me["user_id"].as_str().unwrap_or_default().to_string();
        let opened = self.call("apps.connections.open", &self.app, &json!({}))?;
        let url = opened["url"]
            .as_str()
            .ok_or("Slack did not say where to connect")?;
        let mut socket = connect("Slack", url)?;
        (to.connected)(format!("@{}", me["user"].as_str().unwrap_or("bot")));
        while !to.stop.load(Ordering::Relaxed) {
            let Some(text) = read("Slack", &mut socket)? else {
                continue;
            };
            let Ok(envelope) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            // Slack ends each connection after a while and says so; a new one picks up from there.
            if !self.envelope(&mut socket, &envelope, to)? {
                break;
            }
        }
        let _ = socket.close(None);
        Ok(())
    }

    fn send(&self, chat: &str, text: &str, buttons: &[Button]) -> Result<Option<String>, Error> {
        let mut body = json!({"channel": chat, "text": text});
        if !buttons.is_empty() {
            let elements: Vec<Value> = buttons
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    json!({
                        "type": "button",
                        "text": {"type": "plain_text", "text": b.label},
                        "value": b.reply,
                        "action_id": format!("answer-{i}"),
                    })
                })
                .collect();
            body["blocks"] = json!([
                {"type": "section", "text": {"type": "mrkdwn", "text": text}},
                {"type": "actions", "elements": elements},
            ]);
        }
        let sent = self.call("chat.postMessage", &self.bot, &body)?;
        Ok(sent["ts"].as_str().map(str::to_string))
    }

    fn send_file(&self, chat: &str, name: &str, bytes: &[u8], caption: &str) -> Result<(), Error> {
        let length = bytes.len().to_string();
        let upload = self.call_form(
            "files.getUploadURLExternal",
            &[("filename", name), ("length", &length)],
        )?;
        let (Some(url), Some(id)) = (upload["upload_url"].as_str(), upload["file_id"].as_str())
        else {
            return Err("Slack gave no place to upload the file".into());
        };
        let response = self
            .http
            .post(url)
            .header("Content-Type", mime(name))
            .send(bytes)
            .map_err(|e| failed("Slack could not be reached", e, &self.bot))?;
        if !response.status().is_success() {
            return Err(format!("Slack refused the file: {}", response.status()).into());
        }
        let files = json!([{"id": id, "title": name}]).to_string();
        self.call_form(
            "files.completeUploadExternal",
            &[
                ("files", &files),
                ("channel_id", chat),
                ("initial_comment", caption),
            ],
        )
        .map(drop)
    }

    fn edit(&self, chat: &str, id: &str, text: &str) -> Result<(), Error> {
        self.call(
            "chat.update",
            &self.bot,
            &json!({"channel": chat, "ts": id, "text": text}),
        )
        .map(drop)
    }
}

/// The answer to a Web API call, or the reason Slack gave for refusing it.
fn result(response: ureq::http::Response<ureq::Body>) -> Result<Value, Error> {
    let body = json("Slack", response)?;
    if body["ok"].as_bool() == Some(true) {
        Ok(body)
    } else {
        let reason = body["error"].as_str().unwrap_or("no reason given");
        Err(format!("Slack refused the request: {reason}").into())
    }
}
