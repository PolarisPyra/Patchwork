//! What the core asks of the running gateway, which lives in its own crate and is handed to the
//! core with [`Core::set_chat_app_hooks`]. Without one, as on a host with no window, there are no
//! chat apps: nothing is sent and no status is shown.

// Rust guideline compliant 2026-11-27

use std::sync::Arc;

use super::platform::Platform;
use super::types::Status;
use crate::chats::Message;
use crate::events::ChatEventEnvelope;
use crate::{Core, Error};

/// What the core is told when no chat app can take a file.
const NO_CHAT_APPS: &str = "No chat app is connected";

/// The running gateway, as the core calls it.
pub trait ChatAppHooks: Send + Sync {
    /// Hears of a chat event, which may go out to the chat's conversations.
    fn observe(&self, _core: &Core, _event: &ChatEventEnvelope) {}

    /// Shows the message the person sent to chat `chat_id` in its conversations, except `from`,
    /// the one it came from.
    fn mirror(&self, _chat_id: String, _message: &Message, _from: Option<(Platform, &str)>) {}

    /// Stops every connection and opens the ones the settings turn on.
    fn restart(&self, _core: &Arc<Core>) {}

    /// Where each chat app's connection stands.
    fn status(&self) -> Vec<(Platform, Status)> {
        Vec::new()
    }

    /// A new one-time pairing code, and when it stops working, in epoch milliseconds.
    fn new_pairing_code(&self) -> (String, u64) {
        (String::new(), 0)
    }

    /// Sends the diff of chat `chat_id`'s last turn to `conversation`, and says what happened.
    fn send_diff(
        &self,
        _core: &Core,
        _platform: Platform,
        _conversation: &str,
        _chat_id: Option<&str>,
    ) -> Option<String> {
        None
    }

    /// Sends the file `name` to the conversations of chat `chat_id`, and says how many took it.
    ///
    /// # Errors
    ///
    /// Returns an error when no conversation took it.
    fn send_file_to_chats(
        &self,
        _core: &Core,
        _chat_id: &str,
        _name: &str,
        _bytes: &[u8],
        _caption: &str,
    ) -> Result<usize, Error> {
        Err(NO_CHAT_APPS.into())
    }

    /// Sends a picture to the conversations of chat `chat_id`, and says what it was sent as and
    /// how many took it.
    ///
    /// # Errors
    ///
    /// Returns an error when no conversation took it.
    fn send_picture_to_chats(
        &self,
        _core: &Core,
        _chat_id: &str,
        _stem: &str,
        _bytes: Vec<u8>,
        _caption: &str,
    ) -> Result<(String, usize), Error> {
        Err(NO_CHAT_APPS.into())
    }

    /// Notes that chat `caller`'s agent is working with chat `target` this turn.
    fn chat_app_visits(&self, _core: &Core, _caller: &str, _target: &str) {}

    /// `text` with what a chat app's conversation should know before chat `chat_id` reads it.
    fn chat_app_context(&self, _core: &Core, _chat_id: &str, text: String) -> String {
        text
    }

    /// Takes back the notices sent for chat `chat_id`'s usage limit reset.
    fn drop_reset_notices(&self, _core: &Core, _chat_id: &str) {}

    /// Answers the request chat `chat_id` is waiting on, for the dispatcher chat `dispatcher`.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat waits on nothing or the answer cannot be given.
    fn answer_for_dispatcher(
        &self,
        _core: &Arc<Core>,
        _dispatcher: &str,
        _chat_id: &str,
        _text: &str,
    ) -> Result<String, Error> {
        Err(NO_CHAT_APPS.into())
    }
}

/// The hooks used before any are set: no chat apps.
struct NoChatApps;

impl ChatAppHooks for NoChatApps {}

impl Core {
    /// Hands the core the running gateway. Only the first call takes effect.
    pub fn set_chat_app_hooks(&self, hooks: Arc<dyn ChatAppHooks>) {
        let _ = self.chat_app_hooks.set(hooks);
    }

    /// The running gateway, or none.
    fn chat_apps(&self) -> &dyn ChatAppHooks {
        self.chat_app_hooks
            .get()
            .map_or(&NoChatApps as &dyn ChatAppHooks, |hooks| hooks.as_ref())
    }

    /// Passes a chat event on to the chat apps.
    pub(crate) fn chat_apps_observe(&self, event: &ChatEventEnvelope) {
        self.chat_apps().observe(self, event);
    }

    /// Shows a message the person sent in its chat's conversations, except `from`.
    pub(crate) fn mirror_to_chat_apps(
        &self,
        chat_id: String,
        message: &Message,
        from: Option<(Platform, &str)>,
    ) {
        self.chat_apps().mirror(chat_id, message, from);
    }

    /// Stops every connection and opens the ones the settings turn on. This reads the system
    /// credential store, which can block.
    pub fn restart_gateway(self: &Arc<Self>) {
        self.chat_apps().restart(self);
    }

    /// Where each chat app's connection stands.
    #[must_use]
    pub fn gateway_status(&self) -> Vec<(Platform, Status)> {
        self.chat_apps().status()
    }

    /// A new one-time code that pairs whoever sends it to a connected bot, and when it stops
    /// working, in epoch milliseconds.
    #[must_use]
    pub fn new_pairing_code(&self) -> (String, u64) {
        self.chat_apps().new_pairing_code()
    }

    /// Sends the diff of chat `chat_id`'s last turn to `conversation`, and says what happened.
    pub(crate) fn send_diff(
        &self,
        platform: Platform,
        conversation: &str,
        chat_id: Option<&str>,
    ) -> Option<String> {
        self.chat_apps()
            .send_diff(self, platform, conversation, chat_id)
    }

    /// Sends the file `name` to the conversations of chat `chat_id`.
    pub(crate) fn send_file_to_chats(
        &self,
        chat_id: &str,
        name: &str,
        bytes: &[u8],
        caption: &str,
    ) -> Result<usize, Error> {
        self.chat_apps()
            .send_file_to_chats(self, chat_id, name, bytes, caption)
    }

    /// Sends a picture to the conversations of chat `chat_id`.
    pub(crate) fn send_picture_to_chats(
        &self,
        chat_id: &str,
        stem: &str,
        bytes: Vec<u8>,
        caption: &str,
    ) -> Result<(String, usize), Error> {
        self.chat_apps()
            .send_picture_to_chats(self, chat_id, stem, bytes, caption)
    }

    /// Notes that chat `caller`'s agent is working with chat `target` this turn.
    pub(crate) fn chat_app_visits(&self, caller: &str, target: &str) {
        self.chat_apps().chat_app_visits(self, caller, target);
    }

    /// `text` with what a chat app's conversation should know before chat `chat_id` reads it.
    pub(crate) fn chat_app_context(&self, chat_id: &str, text: String) -> String {
        self.chat_apps().chat_app_context(self, chat_id, text)
    }

    /// Takes back the notices sent for chat `chat_id`'s usage limit reset.
    pub(crate) fn drop_reset_notices(&self, chat_id: &str) {
        self.chat_apps().drop_reset_notices(self, chat_id);
    }

    /// Answers the request chat `chat_id` is waiting on, for the dispatcher chat `dispatcher`.
    pub(crate) fn answer_for_dispatcher(
        self: &Arc<Self>,
        dispatcher: &str,
        chat_id: &str,
        text: &str,
    ) -> Result<String, Error> {
        self.chat_apps()
            .answer_for_dispatcher(self, dispatcher, chat_id, text)
    }
}
