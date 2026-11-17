//! Telling a conversation about the chats its dispatcher started or was asked to watch: what
//! they ask, and their reply when they finish. Also the dispatcher's way to pass an answer on.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::sync::Arc;

use super::inbound::{DENY, answers};
use super::notify::Notice;
use super::outbound::{clip, request};
use super::platform::Platform;
use super::types::Button;
use crate::events::AgentEvent;
use crate::threads::{Approval, ApprovalStatus, MessageStatus, Thread};
use crate::{Core, Error};

/// The most characters of a watched chat's reply a conversation is sent.
const REPLY_LIMIT: usize = 1500;

impl Core {
    /// Tells the conversations watching `thread_id` about `event`, and says whether any watch it,
    /// so the pairings are not told a second time.
    pub(super) fn tell_watchers(&self, thread_id: &str, event: &AgentEvent) -> bool {
        if !matches!(
            event,
            AgentEvent::ApprovalRequested { .. }
                | AgentEvent::ApprovalResolved { .. }
                | AgentEvent::TurnCompleted { .. }
        ) {
            return false;
        }
        let watchers = self.watchers(thread_id);
        if watchers.is_empty() {
            return false;
        }
        let Some(thread) = self.store.thread(thread_id) else {
            return true;
        };
        match event {
            AgentEvent::ApprovalRequested { approval, .. }
                if !approval.auto && approval.status == ApprovalStatus::Pending =>
            {
                for w in &watchers {
                    self.show_request(&thread, approval, w.platform, &w.chat);
                }
            }
            AgentEvent::ApprovalResolved {
                approval_id,
                allowed,
                answers,
                auto: false,
                ..
            } => self.close_notices(approval_id, *allowed, !answers.is_empty()),
            AgentEvent::TurnCompleted {
                message_id,
                status,
                error,
            } => {
                self.unwatch(thread_id);
                let Some(reply) = self.reply(thread_id, message_id, *status, error.clone()) else {
                    return true;
                };
                let how = if *status == MessageStatus::Complete {
                    "finished"
                } else {
                    "stopped"
                };
                let text = format!(
                    "{} {how}:\n\n{}",
                    self.heading(&thread),
                    clip(&reply, REPLY_LIMIT)
                );
                let open = Button {
                    label: "Continue here".to_string(),
                    reply: format!("/open {thread_id}"),
                };
                for w in &watchers {
                    self.say_with(w.platform, &w.chat, &text, std::slice::from_ref(&open));
                }
            }
            _ => {}
        }
        true
    }

    /// Shows the request of `thread`, which `chat` does not continue, in `chat` with buttons that
    /// answer it, once per request.
    pub(super) fn show_request(
        &self,
        thread: &Thread,
        approval: &Approval,
        platform: Platform,
        chat: &str,
    ) {
        let shown = self
            .gateway
            .lock()
            .notices
            .iter()
            .any(|n| n.platform == platform && n.chat == chat && n.approval_id == approval.id);
        if shown {
            return;
        }
        let (text, buttons) = request(approval);
        let text = format!("{} asks:\n\n{text}", self.heading(thread));
        let buttons: Vec<Button> = buttons
            .into_iter()
            .map(|b| Button {
                label: b.label,
                reply: format!("/answer {} {}", approval.id, b.reply),
            })
            .collect();
        self.say_with(platform, chat, &text, &buttons);
        if approval.secret.is_none() {
            self.gateway.lock().notices.push(Notice {
                platform,
                chat: chat.to_string(),
                thread_id: thread.id.clone(),
                approval_id: approval.id.clone(),
            });
        }
    }

    /// Passes `text` on as the answer to what `thread_id` waits for, for the dispatcher
    /// `dispatcher`, and returns what to tell its agent.
    ///
    /// A question is answered. A request to run or change something can only be denied: to
    /// allow it is the person's, so it is shown in the dispatcher's conversations instead.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat waits for nothing or for a secret, the answer is empty, or
    /// the chat does not take it.
    pub(crate) fn answer_for_dispatcher(
        self: &Arc<Self>,
        dispatcher: &str,
        thread_id: &str,
        text: &str,
    ) -> Result<String, Error> {
        let approval = self
            .waiting_approval(thread_id)
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
            self.respond_approval(thread_id, &approval.id, true, answers, None)?;
            return Ok("Answered. The chat carries on.".into());
        }
        if DENY.contains(&text.to_lowercase().as_str()) {
            self.respond_approval(thread_id, &approval.id, false, HashMap::new(), None)?;
            return Ok("Denied.".into());
        }
        let thread = self
            .store
            .thread(thread_id)
            .ok_or("That chat is no longer here")?;
        let chats = self.bound_chats(dispatcher);
        if chats.is_empty() {
            return Err("Only the person can allow that, in Patchwork on their computer".into());
        }
        for b in &chats {
            self.show_request(&thread, &approval, b.platform, &b.chat);
        }
        Ok(
            "Only the person can allow that. Patchwork has shown them the request with buttons \
             to answer it, so say nothing more about allowing it."
                .into(),
        )
    }
}
