//! The gateway's settings, and what passes between it and the chat apps.

// Rust guideline compliant 2026-10-09

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};

use super::platform::Platform;
use crate::Error;
use crate::threads::AccessMode;

/// Which chat apps Patchwork answers on, and what the chats it starts there may do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GatewaySettings {
    pub platforms: HashMap<Platform, PlatformSettings>,
    /// What chats started from a chat app may do without asking.
    pub access: AccessMode,
    /// The project chats started from a chat app open in; none starts them as chats.
    pub project_id: Option<String>,
}

impl Default for GatewaySettings {
    fn default() -> Self {
        Self {
            platforms: HashMap::new(),
            access: AccessMode::Supervised,
            project_id: None,
        }
    }
}

impl GatewaySettings {
    /// The settings of `platform`, empty when it was never set up.
    #[must_use]
    pub fn platform(&self, platform: Platform) -> PlatformSettings {
        self.platforms.get(&platform).cloned().unwrap_or_default()
    }
}

/// One platform's settings. Its secrets are kept in the system credential store instead.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PlatformSettings {
    pub enabled: bool,
    /// The values of its `Platform::settings` fields, by key.
    pub values: BTreeMap<String, String>,
}

impl PlatformSettings {
    /// The value of the setting `key`, empty when it was never set.
    #[must_use]
    pub fn value(&self, key: &str) -> &str {
        self.values.get(key).map_or("", String::as_str)
    }
}

/// Where a platform's connection stands.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Off,
    Connecting,
    /// Connected, as the bot or account named.
    Connected(String),
    Error(String),
}

/// Someone allowed to use Patchwork from a chat app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedUser {
    pub platform: Platform,
    /// Their id on the platform.
    pub id: String,
    pub name: String,
    /// Epoch milliseconds.
    pub paired_at: u64,
    /// The conversation they last wrote from, where a thread can be continued for them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat: Option<String>,
}

/// A conversation on a chat app and the thread it talks to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    pub platform: Platform,
    pub chat: String,
    pub thread_id: String,
}

/// Where a thread stands with the chat apps, for the task list and the thread's menu.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatApps {
    /// The chat app its first message was sent from.
    pub started_on: Option<Platform>,
    /// The conversations that continue it now.
    pub chats: Vec<Binding>,
}

/// A conversation on a chat app a thread can be continued in: a paired person's latest one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatPlace {
    pub platform: Platform,
    pub chat: String,
    /// The paired person's name.
    pub name: String,
}

/// A message that arrived from a chat app.
#[derive(Debug, Clone)]
pub struct Incoming {
    pub platform: Platform,
    /// The conversation it came in, which replies go to.
    pub chat: String,
    /// The sender's id on the platform.
    pub user: String,
    /// The sender's name, for the list of paired users.
    pub name: String,
    pub text: String,
    /// The bytes of the files sent with it; those that are pictures are attached.
    pub files: Vec<Vec<u8>>,
}

/// A button under a message; pressing it sends `reply` as the user's message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    pub label: String,
    pub reply: String,
}

/// What `Connector::listen` reports while it runs.
pub(crate) struct Listener<'a> {
    pub stop: &'a AtomicBool,
    /// Called once connected, with the bot's or account's name.
    pub connected: &'a dyn Fn(String),
    pub inbox: &'a dyn Fn(Incoming),
}

/// A connection to one chat app.
pub(crate) trait Connector: Send + Sync {
    /// Connects and hands each message to `to.inbox` until `to.stop` is set or the connection
    /// ends.
    ///
    /// # Errors
    ///
    /// Returns an error when the platform cannot be reached, refuses the credentials, or drops
    /// the connection.
    fn listen(&self, to: &Listener<'_>) -> Result<(), Error>;

    /// Sends `text` to `chat` with `buttons` under it, where the platform has buttons, and
    /// returns the message's id when it can be edited.
    ///
    /// # Errors
    ///
    /// Returns an error when the platform does not take the message.
    fn send(&self, chat: &str, text: &str, buttons: &[Button]) -> Result<Option<String>, Error>;

    /// Replaces the text of the message `id`, which `send` returned.
    ///
    /// # Errors
    ///
    /// Returns an error when the platform does not take the change.
    fn edit(&self, chat: &str, id: &str, text: &str) -> Result<(), Error>;

    /// Shows that Patchwork is writing in `chat`, where the platform can.
    fn typing(&self, _chat: &str) {}

    /// Whether a turn's progress is worth a message of its own here; where it is not, as where
    /// each message is mail or a paid text, the chat only shows typing.
    fn notes(&self) -> bool {
        true
    }
}
