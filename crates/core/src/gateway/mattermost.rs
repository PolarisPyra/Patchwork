//! Mattermost: posts over the server's websocket, and the REST API to answer. Mattermost buttons
//! need a server Patchwork does not run, so requests are answered in words.

// Rust guideline compliant 2026-10-09

use std::sync::atomic::Ordering;

use serde_json::{Value, json};

use super::net::{client, connect, download, failed, json, multipart, read, write};
use super::platform::Platform;
use super::types::{Button, Connector, Incoming, Listener};
use crate::Error;

pub(crate) struct Mattermost {
    /// The server, without a trailing slash.
    server: String,
    token: String,
    http: ureq::Agent,
}

impl Mattermost {
    pub(crate) fn new(server: String, token: String) -> Self {
        Self {
            server,
            token,
            http: client(),
        }
    }

    fn auth(&self) -> String {
        format!("Bearer {}", self.token)
    }

    /// Calls the REST API at `path` with `method`.
    fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, Error> {
        let url = format!("{}/api/v4{path}", self.server);
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
            response.map_err(|e| failed("Mattermost could not be reached", e, &self.token))?;
        json("Mattermost", response)
    }

    /// The post in a `posted` event, when it was written to the bot.
    fn post(&self, data: &Value, me: &str, name: &str) -> Option<Incoming> {
        let post: Value = serde_json::from_str(data["post"].as_str()?).ok()?;
        let user = post["user_id"].as_str()?;
        if user == me || post["type"].as_str().is_some_and(|t| !t.is_empty()) {
            return None;
        }
        let mention = format!("@{name}");
        let mut text = post["message"].as_str().unwrap_or_default().to_string();
        // Outside a direct message, the bot answers only when it is mentioned.
        if data["channel_type"].as_str() != Some("D") {
            if !text.contains(&mention) {
                return None;
            }
            text = text.replace(&mention, "");
        }
        let auth = self.auth();
        let files = post["file_ids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|id| {
                let url = format!("{}/api/v4/files/{}", self.server, id.as_str()?);
                download(&self.http, &url, Some(&auth)).ok()
            })
            .collect();
        Some(Incoming {
            platform: Platform::Mattermost,
            chat: post["channel_id"].as_str()?.to_string(),
            user: user.to_string(),
            name: data["sender_name"].as_str().unwrap_or(user).to_string(),
            text: text.trim().to_string(),
            files,
        })
    }
}

impl Connector for Mattermost {
    fn listen(&self, to: &Listener<'_>) -> Result<(), Error> {
        let me = self.call("GET", "/users/me", None)?;
        let id = me["id"].as_str().unwrap_or_default().to_string();
        let name = me["username"].as_str().unwrap_or_default().to_string();
        let url = format!(
            "{}/api/v4/websocket",
            self.server
                .replacen("https://", "wss://", 1)
                .replacen("http://", "ws://", 1)
        );
        let mut socket = connect("Mattermost", &url)?;
        write(
            "Mattermost",
            &mut socket,
            &json!({"seq": 1, "action": "authentication_challenge", "data": {"token": self.token}}),
        )?;
        (to.connected)(format!("@{name}"));
        while !to.stop.load(Ordering::Relaxed) {
            let Some(text) = read("Mattermost", &mut socket)? else {
                continue;
            };
            let Ok(event) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            if event["status"].as_str() == Some("FAIL") {
                return Err("Mattermost refused the access token".into());
            }
            if event["event"].as_str() == Some("posted")
                && let Some(message) = self.post(&event["data"], &id, &name)
            {
                (to.inbox)(message);
            }
        }
        let _ = socket.close(None);
        Ok(())
    }

    fn send(&self, chat: &str, text: &str, _buttons: &[Button]) -> Result<Option<String>, Error> {
        let sent = self.call(
            "POST",
            "/posts",
            Some(&json!({"channel_id": chat, "message": text})),
        )?;
        Ok(sent["id"].as_str().map(str::to_string))
    }

    fn send_file(&self, chat: &str, name: &str, bytes: &[u8], caption: &str) -> Result<(), Error> {
        let (kind, body) = multipart(&[("channel_id", chat)], "files", name, bytes);
        let response = self
            .http
            .post(&format!("{}/api/v4/files", self.server))
            .header("Authorization", &self.auth())
            .header("Content-Type", &kind)
            .send(body.as_slice())
            .map_err(|e| failed("Mattermost could not be reached", e, &self.token))?;
        let uploaded = json("Mattermost", response)?;
        let id = uploaded["file_infos"][0]["id"]
            .as_str()
            .ok_or("Mattermost did not keep the file")?;
        self.call(
            "POST",
            "/posts",
            Some(&json!({"channel_id": chat, "message": caption, "file_ids": [id]})),
        )
        .map(drop)
    }

    fn edit(&self, _chat: &str, id: &str, text: &str) -> Result<(), Error> {
        self.call(
            "PUT",
            &format!("/posts/{id}/patch"),
            Some(&json!({"message": text})),
        )
        .map(drop)
    }
}
