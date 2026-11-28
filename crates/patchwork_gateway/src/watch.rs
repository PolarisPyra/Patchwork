//! Telling a conversation about the chats its dispatcher started or was asked to watch: what
//! they ask, and their reply when they finish. Also the dispatcher's way to pass an answer on.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::sync::Arc;

use super::inbound::{DENY, answers};
use super::notify::Notice;
use super::outbound::request;
use patchwork_core::provider::clip;
use super::platform::Platform;
use super::types::Button;
use patchwork_core::events::ChatEvent;
use patchwork_core::chats::{Approval, ApprovalStatus, MessageStatus, Chat};
use patchwork_core::{Core, Error};
#[allow(
    unused_imports,
    reason = "each file takes the core's gateway methods it calls"
)]
use crate::prelude::*;
use crate::hub::HUB;

/// The most characters of a watched chat's reply a conversation is sent.
const REPLY_LIMIT: usize = 300;

/// The core's watch methods the gateway calls.
pub(crate) trait Watch {
    /// Tells the conversations watching `chat_id` about `event`, and says whether any watch it,
    /// so the pairings are not told a second time.
    fn tell_watchers(&self, chat_id: &str, event: &ChatEvent) -> bool;

    /// Shows the request of `chat`, which `chat` does not continue, in `chat` with buttons that
    /// answer it, once per request.
    fn show_request(
        &self,
        chat: &Chat,
        approval: &Approval,
        platform: Platform,
        conversation: &str,
    );

    /// Passes `text` on as the answer to what `chat_id` waits for, for the dispatcher
    /// `dispatcher`, and returns what to tell its agent.
    ///
    /// A question is answered. A request to run or change something can only be denied: to
    /// allow it is the person's, so it is shown in the dispatcher's conversations instead.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat waits for nothing or for a secret, the answer is empty, or
    /// the chat does not take it.
    fn answer_for_dispatcher(
        self: &Arc<Self>,
        dispatcher: &str,
        chat_id: &str,
        text: &str,
    ) -> Result<String, Error>;
}

impl Watch for Core {
    fn tell_watchers(&self, chat_id: &str, event: &ChatEvent) -> bool {
        if !matches!(
            event,
            ChatEvent::ApprovalRequested { .. }
                | ChatEvent::ApprovalResolved { .. }
                | ChatEvent::TurnCompleted { .. }
        ) {
            return false;
        }
        let watchers = self.watchers(chat_id);
        if watchers.is_empty() {
            return false;
        }
        let Some(chat) = self.store().chat(chat_id) else {
            return true;
        };
        match event {
            ChatEvent::ApprovalRequested { approval, .. }
                if !approval.auto && approval.status == ApprovalStatus::Pending =>
            {
                for w in &watchers {
                    self.show_request(&chat, approval, w.platform, &w.conversation);
                }
            }
            ChatEvent::ApprovalResolved {
                approval_id,
                allowed,
                answers,
                auto: false,
                ..
            } => self.close_notices(approval_id, *allowed, !answers.is_empty()),
            ChatEvent::TurnCompleted {
                message_id,
                status,
                error,
            } => {
                self.unwatch(chat_id);
                let Some(reply) = self.reply(chat_id, message_id, *status, error.clone()) else {
                    return true;
                };
                let how = if *status == MessageStatus::Complete {
                    "finished"
                } else {
                    "stopped"
                };
                let text = format!(
                    "{} {how}:\n\n{}",
                    self.heading(&chat),
                    clip(&reply, REPLY_LIMIT)
                );
                for w in &watchers {
                    self.say(w.platform, &w.conversation, &text);
                }
            }
            _ => {}
        }
        true
    }

    fn show_request(
        &self,
        chat: &Chat,
        approval: &Approval,
        platform: Platform,
        conversation: &str,
    ) {
        let shown = HUB.lock().notices.iter().any(|n| {
            n.platform == platform && n.conversation == conversation && n.approval_id == approval.id
        });
        if shown {
            return;
        }
        let (text, buttons) = request(approval);
        let text = format!("{} asks:\n\n{text}", self.heading(chat));
        let buttons: Vec<Button> = buttons
            .into_iter()
            .map(|b| Button {
                label: b.label,
                reply: format!("/answer {} {}", approval.id, b.reply),
            })
            .collect();
        self.say_with(platform, conversation, &text, &buttons);
        if approval.secret.is_none() {
            HUB.lock().notices.push(Notice {
                platform,
                conversation: conversation.to_string(),
                chat_id: chat.id.clone(),
                approval_id: approval.id.clone(),
            });
        }
    }

    fn answer_for_dispatcher(
        self: &Arc<Self>,
        dispatcher: &str,
        chat_id: &str,
        text: &str,
    ) -> Result<String, Error> {
        let approval = self
            .waiting_approval(chat_id)
            .ok_or("That chat is not waiting for an answer")?;
        if approval.secret.is_some() {
            return Err(
                "It asks for a secret, which the person enters in Patchwork on their computer"
                    .into(),
            );
        }
        if !approval.questions.is_empty() {
            let answers = answers(&approval.questions, text);
            if text.is_empty() || answers.is_empty() {
                return Err("Give the person's answer".into());
            }
            self.respond_approval(chat_id, &approval.id, true, answers, None)?;
            return Ok("Answered. The chat carries on.".into());
        }
        if DENY.contains(&text.to_lowercase().as_str()) {
            self.respond_approval(chat_id, &approval.id, false, HashMap::new(), None)?;
            return Ok("Denied.".into());
        }
        let chat = self
            .store()
            .chat(chat_id)
            .ok_or("That chat is no longer here")?;
        let conversations = self.bound_conversations(dispatcher);
        if conversations.is_empty() {
            return Err("Only the person can allow that, in Patchwork on their computer".into());
        }
        for b in &conversations {
            self.show_request(&chat, &approval, b.platform, &b.conversation);
        }
        Ok(
            "Only the person can allow that. Patchwork has shown them the request with buttons \
             to answer it, so say nothing more about allowing it."
                .into(),
        )
    }
}
