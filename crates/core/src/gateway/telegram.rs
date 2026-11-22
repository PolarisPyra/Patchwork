//! Telegram, through the Bot API: a long poll for messages and button presses.

// Rust guideline compliant 2026-10-09

use std::sync::atomic::Ordering;

use serde_json::{Value, json};

use super::net::{client, download, failed, json, multipart};
use super::platform::Platform;
use super::types::{Button, Connector, Incoming, Listener};
use crate::Error;

const API: &str = "https://api.telegram.org";

/// How long each poll waits for a message, in seconds; under the request timeout.
const POLL_SECONDS: u64 = 10;

/// How many buttons share a row; more go one to a row.
const ROW: usize = 2;

pub(crate) struct Telegram {
    token: String,
    http: ureq::Agent,
}

impl Telegram {
    pub(crate) fn new(token: String) -> Self {
        Self {
            token,
            http: client(),
        }
    }

    /// Calls the Bot API method `method` and returns its result.
    fn call(&self, method: &str, body: &Value) -> Result<Value, Error> {
        let url = format!("{API}/bot{}/{method}", self.token);
        let response = self
            .http
            .post(&url)
            .send_json(body)
            .map_err(|e| failed("Telegram could not be reached", e, &self.token))?;
        result(response)
    }

    /// The pictures sent with a message, the largest size of each.
    fn photos(&self, message: &Value) -> Vec<Vec<u8>> {
        let photo = message["photo"].as_array().and_then(|sizes| sizes.last());
        let document = message["document"].as_object().filter(|d| {
            d["mime_type"]
                .as_str()
                .is_some_and(|m| m.starts_with("image/"))
        });
        let ids = photo
            .and_then(|p| p["file_id"].as_str())
            .into_iter()
            .chain(document.and_then(|d| d["file_id"].as_str()));
        ids.filter_map(|id| {
            let file = self.call("getFile", &json!({"file_id": id})).ok()?;
            let path = file["file_path"].as_str()?;
            let url = format!("{API}/file/bot{}/{path}", self.token);
            download(&self.http, &url, None).ok()
        })
        .collect()
    }

    /// The message in an update, as Patchwork takes it.
    fn incoming(&self, update: &Value, me: &str) -> Option<Incoming> {
        if let Some(press) = update.get("callback_query") {
            let _ = self.call(
                "answerCallbackQuery",
                &json!({"callback_query_id": press["id"]}),
            );
            let message = &press["message"];
            // The buttons go once one is pressed, so an answer is not given twice.
            let _ = self.call(
                "editMessageReplyMarkup",
                &json!({
                    "chat_id": message["chat"]["id"],
                    "message_id": message["message_id"],
                    "reply_markup": {"inline_keyboard": []},
                }),
            );
            return Some(Incoming {
                platform: Platform::Telegram,
                id: press["id"].as_str().map(|id| format!("press:{id}")),
                conversation: id_text(&message["chat"]["id"])?,
                sender: id_text(&press["from"]["id"])?,
                name: name(&press["from"]),
                text: press["data"].as_str()?.to_string(),
                files: Vec::new(),
            });
        }
        let message = update.get("message")?;
        let from = &message["from"];
        if from["is_bot"].as_bool() == Some(true) {
            return None;
        }
        let text = message["text"]
            .as_str()
            .or_else(|| message["caption"].as_str())
            .unwrap_or_default()
            .replace(&format!("@{me}"), "");
        Some(Incoming {
            platform: Platform::Telegram,
            id: id_text(&message["message_id"]),
            conversation: id_text(&message["chat"]["id"])?,
            sender: id_text(&from["id"])?,
            name: name(from),
            text: text.trim().to_string(),
            files: self.photos(message),
        })
    }
}

impl Connector for Telegram {
    fn listen(&self, to: &Listener<'_>) -> Result<(), Error> {
        let me = self.call("getMe", &json!({}))?;
        let me = me["username"].as_str().unwrap_or_default().to_string();
        (to.connected)(format!("@{me}"));
        // Messages sent while Patchwork was closed are skipped rather than answered late.
        let skipped = self.call("getUpdates", &json!({"offset": -1, "timeout": 0}))?;
        let mut offset = last_id(&skipped).map_or(0, |id| id + 1);
        while !to.stop.load(Ordering::Relaxed) {
            let updates = self.call(
                "getUpdates",
                &json!({
                    "offset": offset,
                    "timeout": POLL_SECONDS,
                    "allowed_updates": ["message", "callback_query"],
                }),
            )?;
            if let Some(id) = last_id(&updates) {
                offset = offset.max(id + 1);
            }
            for update in updates.as_array().into_iter().flatten() {
                if let Some(message) = self.incoming(update, &me) {
                    (to.inbox)(message);
                }
            }
        }
        Ok(())
    }

    fn send(
        &self,
        conversation: &str,
        text: &str,
        buttons: &[Button],
    ) -> Result<Option<String>, Error> {
        let mut body = json!({"chat_id": conversation, "text": text});
        if !buttons.is_empty() {
            let keys: Vec<Value> = buttons
                .iter()
                .map(|b| json!({"text": b.label, "callback_data": b.reply}))
                .collect();
            let rows: Vec<Value> = if keys.len() <= ROW {
                vec![Value::Array(keys)]
            } else {
                keys.into_iter().map(|k| Value::Array(vec![k])).collect()
            };
            body["reply_markup"] = json!({"inline_keyboard": rows});
        }
        let sent = self.call("sendMessage", &body)?;
        Ok(sent["message_id"].as_i64().map(|id| id.to_string()))
    }

    fn send_file(
        &self,
        conversation: &str,
        name: &str,
        bytes: &[u8],
        caption: &str,
    ) -> Result<(), Error> {
        let (kind, body) = multipart(
            &[("chat_id", conversation), ("caption", caption)],
            "document",
            name,
            bytes,
        );
        let url = format!("{API}/bot{}/sendDocument", self.token);
        let response = self
            .http
            .post(&url)
            .header("Content-Type", &kind)
            .send(body.as_slice())
            .map_err(|e| failed("Telegram could not be reached", e, &self.token))?;
        result(response).map(drop)
    }

    fn edit(&self, conversation: &str, id: &str, text: &str) -> Result<(), Error> {
        let id: i64 = id.parse().map_err(|_bad| "Not a Telegram message id")?;
        self.call(
            "editMessageText",
            &json!({"chat_id": conversation, "message_id": id, "text": text}),
        )
        .map(drop)
    }

    fn remove(&self, conversation: &str, id: &str) -> Result<(), Error> {
        let id: i64 = id.parse().map_err(|_bad| "Not a Telegram message id")?;
        self.call(
            "deleteMessage",
            &json!({"chat_id": conversation, "message_id": id}),
        )
        .map(drop)
    }

    fn typing(&self, conversation: &str) {
        let _ = self.call(
            "sendChatAction",
            &json!({"chat_id": conversation, "action": "typing"}),
        );
    }
}

/// The id of the last update in a `getUpdates` result.
fn last_id(updates: &Value) -> Option<i64> {
    updates.as_array()?.last()?["update_id"].as_i64()
}

/// A Telegram id, which is a number, as text.
fn id_text(id: &Value) -> Option<String> {
    id.as_i64().map(|id| id.to_string())
}

/// What to call a Telegram user: their username, or their first name.
fn name(user: &Value) -> String {
    user["username"]
        .as_str()
        .map(|u| format!("@{u}"))
        .or_else(|| user["first_name"].as_str().map(str::to_string))
        .unwrap_or_default()
}

/// The result of a Bot API call, or the reason Telegram gave for refusing it.
fn result(response: ureq::http::Response<ureq::Body>) -> Result<Value, Error> {
    let body = json("Telegram", response)?;
    if body["ok"].as_bool() == Some(true) {
        Ok(body["result"].clone())
    } else {
        let reason = body["description"].as_str().unwrap_or("no reason given");
        Err(format!("Telegram refused the request: {reason}").into())
    }
}
