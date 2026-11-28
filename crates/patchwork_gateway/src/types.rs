//! What passes between the gateway and the chat apps: the messages that arrive, the buttons
//! under the ones sent, and the connection each chat app is reached over.
//!
//! The gateway's settings and records are the core's, and are re-exported here.

// Rust guideline compliant 2026-10-09

use std::sync::atomic::AtomicBool;

use patchwork_platform::Error;

pub use patchwork_core::gateway::{Binding, ChatApps, ChatPlace, PlatformSettings, Status};

use super::platform::Platform;

/// A message that arrived from a chat app.
#[derive(Debug, Clone)]
pub struct Incoming {
    pub platform: Platform,
    /// The platform's own id of the message or button press, when it gives one. A message that
    /// arrives again under the same id is taken once.
    pub id: Option<String>,
    /// The conversation it came in, which replies go to.
    pub conversation: String,
    /// The sender's id on the platform.
    pub sender: String,
    /// The sender's name, for the list of pairings.
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
    fn send(
        &self,
        conversation: &str,
        text: &str,
        buttons: &[Button],
    ) -> Result<Option<String>, Error>;

    /// Replaces the text of the message `id`, which `send` returned.
    ///
    /// # Errors
    ///
    /// Returns an error when the platform does not take the change.
    fn edit(&self, conversation: &str, id: &str, text: &str) -> Result<(), Error>;

    /// Sends the file `name` holding `bytes` to `chat`, with `caption` beside it.
    ///
    /// # Errors
    ///
    /// Returns an error when the platform takes no files or does not take this one.
    fn send_file(
        &self,
        _conversation: &str,
        _name: &str,
        _bytes: &[u8],
        _caption: &str,
    ) -> Result<(), Error> {
        Err("This chat app takes no files from Patchwork".into())
    }

    /// Deletes the messages in `chat` that the platform lets Patchwork delete, and says whether
    /// that was all of them.
    ///
    /// # Errors
    ///
    /// Returns an error when the platform deletes nothing for Patchwork or stops partway.
    fn clear(&self, _conversation: &str) -> Result<bool, Error> {
        Err("This chat app does not let Patchwork delete messages".into())
    }

    /// Deletes the message `id`, which `send` returned.
    ///
    /// # Errors
    ///
    /// Returns an error when the platform does not delete it.
    fn remove(&self, _conversation: &str, _id: &str) -> Result<(), Error> {
        Err("This chat app does not let Patchwork delete messages".into())
    }

    /// Shows that Patchwork is writing in `chat`, where the platform can.
    fn typing(&self, _conversation: &str) {}

    /// Whether a reply is shown here while it is written, in a message of its own; where it is
    /// not, as where each message is mail or a paid text, the chat only shows typing.
    fn notes(&self) -> bool {
        true
    }
}
