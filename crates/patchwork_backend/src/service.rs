//! The chat operations every surface calls: the window, the pairing server, the chat tools
//! agents drive other chats with, and a chat app's dispatcher.
//!
//! Each operation is written once here, with what it tells the rest of the app: a chat made,
//! changed or deleted is announced as a chat event, so the window, chat apps and paired devices
//! all hear of it the same way whichever surface did it. What a surface checks before it acts,
//! such as a chat tool refusing to message a chat that is replying, stays with that surface.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::sync::Arc;

use crate::{Error, Core};
use crate::chats::{AccessMode, Attachment, Chat, NewChat, Message};
use crate::events::{ChatEvent, ChatEventEnvelope};
use crate::provider::AgentKind;
use crate::permissions::RuleRef;

/// The chat operations, over one core. Get it with [`Core::chats`].
#[derive(Debug, Clone, Copy)]
pub struct Chats<'a> {
    core: &'a Arc<Core>,
}

impl Core {
    /// The chat operations every surface calls.
    #[must_use]
    pub fn chats(self: &Arc<Self>) -> Chats<'_> {
        Chats { core: self }
    }
}

impl Chats<'_> {
    /// Tells every observer that `event` happened to chat `chat_id`.
    fn announce(self, chat_id: &str, event: ChatEvent) {
        self.core.emit(ChatEventEnvelope {
            chat_id: chat_id.to_string(),
            event,
        });
    }
}

impl Chats<'_> {
    /// Makes a chat from `new` and announces it.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown or the chat cannot be saved.
    pub fn create(self, new: NewChat) -> Result<Chat, Error> {
        self.create_linked(new, |_| Ok(()))
    }

    /// Makes a chat from `new`, runs `link` on it, and announces it.
    ///
    /// `link` saves what the chat's first turn must find, such as the agent whose run it is,
    /// before anyone hears of the chat. A chat that cannot be linked is deleted unannounced.
    /// Returns the chat as saved after `link`.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat cannot be made or `link` fails.
    pub fn create_linked(
        self,
        new: NewChat,
        link: impl FnOnce(&Chat) -> Result<(), Error>,
    ) -> Result<Chat, Error> {
        let made = self.core.create_chat(new)?;
        if let Err(e) = link(&made) {
            self.core.delete_chat(&made.id);
            return Err(e);
        }
        let chat = self.core.store.chat(&made.id).unwrap_or(made);
        self.announce(&chat.id, ChatEvent::ChatCreated { chat: chat.clone() });
        Ok(chat)
    }

    /// Makes a chat from `new` and sends it `prompt` with `attachments` as its first message.
    ///
    /// `link` runs as in [`Self::create_linked`]. A chat that cannot be linked or sent to leaves
    /// nothing behind. Returns the chat as saved after the send.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat cannot be made, `link` fails, or the message cannot be
    /// sent.
    pub fn start(
        self,
        new: NewChat,
        prompt: String,
        attachments: Vec<Attachment>,
        link: impl FnOnce(&Chat) -> Result<(), Error>,
    ) -> Result<Chat, Error> {
        let chat = self.create_linked(new, link)?;
        let id = chat.id.clone();
        if let Err(e) = self.send(&id, prompt, attachments, false) {
            self.discard(&id);
            return Err(e);
        }
        Ok(self.core.store.chat(&id).unwrap_or(chat))
    }

    /// Deletes a chat that was announced but could not start, and announces that it is gone.
    pub fn discard(self, chat_id: &str) {
        self.core.delete_chat(chat_id);
        self.announce(chat_id, ChatEvent::ChatDeleted);
    }

    /// Deletes a chat for good, with its subagents' chats, and announces it.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat is still there afterwards; the log says why.
    pub fn delete(self, chat_id: &str) -> Result<(), Error> {
        self.core.delete_chat(chat_id);
        if self.core.store.chat(chat_id).is_some() {
            return Err("Patchwork could not delete that chat; its log says why".into());
        }
        self.announce(chat_id, ChatEvent::ChatDeleted);
        Ok(())
    }

    /// Renames a chat.
    ///
    /// # Errors
    ///
    /// Returns an error when the title is empty, the chat is unknown, or it cannot be saved.
    pub fn rename(self, chat_id: &str, title: &str) -> Result<(), Error> {
        self.core.rename_chat(chat_id, title)?;
        self.announce(chat_id, ChatEvent::OptionsChanged);
        Ok(())
    }

    /// Archives a chat, which stops its turn, or brings it back to the list.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat is unknown or the change cannot be saved.
    pub fn archive(self, chat_id: &str, archived: bool) -> Result<(), Error> {
        self.core.set_chat_archived(chat_id, archived)?;
        self.announce(chat_id, ChatEvent::OptionsChanged);
        Ok(())
    }

    /// Moves a chat to `agent` and `model`; the agent only changes before the first turn.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat is unknown, its agent can no longer change, or the change
    /// cannot be saved.
    pub fn set_model(
        self,
        chat_id: &str,
        agent: AgentKind,
        model: Option<String>,
    ) -> Result<(), Error> {
        self.core.set_chat_model(chat_id, agent, model)?;
        self.announce(chat_id, ChatEvent::OptionsChanged);
        Ok(())
    }

    /// Sets a chat's effort, or its agent's default with None, from its next turn.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat is unknown or the change cannot be saved.
    pub fn set_effort(self, chat_id: &str, effort: Option<&str>) -> Result<(), Error> {
        self.core.set_chat_effort(chat_id, effort)?;
        self.announce(chat_id, ChatEvent::OptionsChanged);
        Ok(())
    }

    /// Sets what a chat's agent may do unasked, which a running turn follows from now on.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat is unknown or the change cannot be saved.
    pub fn set_access_mode(self, chat_id: &str, mode: AccessMode) -> Result<(), Error> {
        self.core.set_chat_access_mode(chat_id, mode)?;
        self.announce(chat_id, ChatEvent::OptionsChanged);
        Ok(())
    }
}

impl Chats<'_> {
    /// Sends `text` with `attachments` to a chat: it starts a turn, steers the running one, or
    /// with `queue` waits behind it.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat is unknown or archived, or its agent cannot start.
    pub fn send(
        self,
        chat_id: &str,
        text: String,
        attachments: Vec<Attachment>,
        queue: bool,
    ) -> Result<Message, Error> {
        self.core
            .send_message(chat_id.to_string(), text, attachments, queue)
    }

    /// Sends the queued message `message_id` into the running turn now.
    ///
    /// # Errors
    ///
    /// Returns an error when the message is gone or the agent cannot take it now.
    pub fn steer(self, chat_id: &str, message_id: &str) -> Result<(), Error> {
        self.core.steer_queued(chat_id, message_id)
    }

    /// Stops a chat's running turn, as the person's stop button does.
    pub fn stop(self, chat_id: &str) {
        self.core.cancel_turn(chat_id);
    }

    /// Allows or denies approval `approval_id`, with `answers` to its questions by id, and with
    /// `rule` saved to answer the like of it from now on.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat has no such approval waiting.
    pub fn answer(
        self,
        chat_id: &str,
        approval_id: &str,
        allow: bool,
        answers: HashMap<String, Vec<String>>,
        rule: Option<RuleRef>,
    ) -> Result<(), Error> {
        self.core
            .respond_approval(chat_id, approval_id, allow, answers, rule)
    }
}
