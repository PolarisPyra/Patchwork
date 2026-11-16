//! SMS through Twilio: Patchwork checks the number's incoming texts every few seconds, so it
//! needs no public address for a webhook, and answers through the Messages API. Texts have no
//! buttons or edits.

// Rust guideline compliant 2026-10-09

use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::Value;

use super::hub::pause;
use super::net::{client, encode, failed, json};
use super::platform::Platform;
use super::types::{Button, Connector, Incoming, Listener};
use crate::Error;

/// How often Patchwork checks for new texts.
const POLL: Duration = Duration::from_secs(5);

/// How many of the newest texts each check reads.
const PAGE: usize = 20;

pub(crate) struct Sms {
    /// The Messages API of the account.
    url: String,
    /// The number texts arrive at and are sent from.
    number: String,
    /// The `Authorization` header: the account SID and auth token.
    auth: String,
    http: ureq::Agent,
    /// The texts the last check read; none before the first check.
    seen: Mutex<Option<HashSet<String>>>,
}

impl Sms {
    pub(crate) fn new(account: &str, number: String, token: &str) -> Self {
        Self {
            url: format!(
                "https://api.twilio.com/2010-04-01/Accounts/{}/Messages.json",
                encode(account)
            ),
            number,
            auth: format!("Basic {}", STANDARD.encode(format!("{account}:{token}"))),
            http: client(),
            seen: Mutex::new(None),
        }
    }

    /// The newest texts sent to the number.
    fn recent(&self) -> Result<Vec<Value>, Error> {
        let response = self
            .http
            .get(&format!(
                "{}?To={}&PageSize={PAGE}",
                self.url,
                encode(&self.number)
            ))
            .header("Authorization", &self.auth)
            .call()
            .map_err(|e| failed("Twilio could not be reached", e, ""))?;
        let body = json("Twilio", response)?;
        Ok(body["messages"].as_array().cloned().unwrap_or_default())
    }
}

impl Connector for Sms {
    fn listen(&self, to: &Listener<'_>) -> Result<(), Error> {
        let mut first = true;
        while !to.stop.load(Ordering::Relaxed) {
            let texts = self.recent()?;
            if first {
                (to.connected)(self.number.clone());
                first = false;
            }
            let ids: HashSet<String> = texts
                .iter()
                .filter_map(|t| t["sid"].as_str().map(str::to_string))
                .collect();
            let before = self
                .seen
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .replace(ids);
            // The first check only learns which texts were already there.
            if let Some(before) = before {
                for text in texts.iter().rev() {
                    let new = text["sid"].as_str().is_some_and(|id| !before.contains(id));
                    if new && let Some(message) = incoming(text) {
                        (to.inbox)(message);
                    }
                }
            }
            pause(POLL, to.stop);
        }
        Ok(())
    }

    fn send(&self, chat: &str, text: &str, _buttons: &[Button]) -> Result<Option<String>, Error> {
        let response = self
            .http
            .post(&self.url)
            .header("Authorization", &self.auth)
            .send_form([("From", self.number.as_str()), ("To", chat), ("Body", text)])
            .map_err(|e| failed("Twilio could not be reached", e, ""))?;
        json("Twilio", response)?;
        Ok(None)
    }

    fn edit(&self, _chat: &str, _id: &str, _text: &str) -> Result<(), Error> {
        Err("Texts cannot be edited".into())
    }

    fn notes(&self) -> bool {
        false
    }
}

/// The message in an incoming text.
fn incoming(text: &Value) -> Option<Incoming> {
    if text["direction"] != "inbound" {
        return None;
    }
    let from = text["from"].as_str()?;
    let body = text["body"].as_str().unwrap_or_default().trim();
    if body.is_empty() {
        return None;
    }
    Some(Incoming {
        platform: Platform::Sms,
        chat: from.to_string(),
        user: from.to_string(),
        name: from.to_string(),
        text: body.to_string(),
        files: Vec::new(),
    })
}
