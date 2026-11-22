//! The messages queued behind a running turn: taking one back, editing it, sending it later or
//! as a steer.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::sync::Arc;

use crate::orchestration::{Delivery, FollowUpInput, TurnRequest};

use crate::events::AgentEvent;
use crate::chats::{Attachment, Message, StepStatus};
use crate::{orchestration, provider};
use crate::Core;

impl Core {
    /// Takes back a message sent mid-turn that has not started yet, so it can be edited.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat is unknown, when the message was already sent, or when the
    /// removal cannot be saved.
    pub fn remove_queued(&self, chat_id: &str, message_id: &str) -> Result<(), Error> {
        let chat = self.chat(chat_id)?;
        let Some(message) = chat.messages.iter().find(|m| m.id == message_id) else {
            // Makes startup cleanup safe to retry if an app refresh overlaps another cleanup.
            return Ok(());
        };
        if !message.queued {
            return Err("That message has already been sent".into());
        }
        let running = self.runner.is_running(chat_id);
        let withdrawn = self.runner.withdraw(chat_id, message_id);
        if running && !withdrawn {
            return Err("That message has already been sent".into());
        }
        orchestration::publish(
            self,
            chat_id,
            AgentEvent::MessageRemoved {
                message_id: message_id.into(),
            },
        );
        Ok(())
    }

    /// Updates a queued message without changing its position or id.
    ///
    /// # Errors
    ///
    /// Returns an error when an attachment image cannot be read or is over 5 MB, when the chat is
    /// unknown, when the message is no longer queued, or when the edit cannot be saved.
    pub fn edit_queued(
        &self,
        chat_id: &str,
        message_id: &str,
        text: String,
        attachments: Vec<Attachment>,
    ) -> Result<(), Error> {
        let images = provider::load_images(&attachments)?;
        let chat = self.chat(chat_id)?;
        let original = chat
            .messages
            .iter()
            .find(|m| m.id == message_id && m.queued)
            .ok_or("That message is no longer queued")?;
        // A message the running turn does not hold was queued before it started: after a Stop,
        // or before a restart. It waits in the chat alone, so the edit below is all of it.
        if self.runner.is_running(chat_id)
            && !self.runner.edit_queued(
                chat_id,
                FollowUpInput {
                    message_id,
                    text: &text,
                    attachments: &attachments,
                    images: &images,
                    merged: &[],
                },
            )
            && self.runner.is_handed_over(message_id)
        {
            return Err("That message has already been sent".into());
        }
        let mut edited = original.as_ref().clone();
        edited.text = text;
        edited.attachments = attachments;
        self.store.update_chat(chat_id, |t| {
            if let Some(m) = t
                .messages
                .iter_mut()
                .find(|m| m.id == message_id && m.queued)
                .map(Arc::make_mut)
            {
                m.text.clone_from(&edited.text);
                m.attachments.clone_from(&edited.attachments);
            }
        })?;
        orchestration::publish(self, chat_id, AgentEvent::MessageEdited { message: edited });
        Ok(())
    }

    /// Sends the chat's next queued message once its turn has ended, in the background.
    pub(crate) fn send_queued_later(self: &Arc<Self>, chat_id: &str) {
        let core = Arc::clone(self);
        let chat_id = chat_id.to_string();
        std::thread::spawn(move || {
            let queued = core
                .store
                .chat(&chat_id)
                .is_some_and(|t| t.messages.iter().any(|m| m.queued));
            if queued && let Err(e) = core.resume_queued(&chat_id) {
                log::warn!(error:% = e; "failed to send queued messages: {{error}}");
            }
        });
    }

    /// Sends the oldest queued message: to the idle agent when one is up, as a normal message
    /// would go, otherwise in a new turn. The rest wait, and each turn's end sends the next.
    ///
    /// # Errors
    ///
    /// Returns an error when a turn is already running, when the chat is unknown or has no queued
    /// messages, when an attachment image cannot be read, or when the queue cannot be saved or the
    /// turn cannot be started.
    pub fn resume_queued(self: &Arc<Self>, chat_id: &str) -> Result<(), Error> {
        if self.runner.is_running(chat_id) {
            return Err("This turn is already running".into());
        }
        let chat = self.chat(chat_id)?;
        let Some(next) = chat.messages.into_iter().find(|m| m.queued) else {
            return Err("There are no queued messages to resume".into());
        };
        let queued = [next];
        let next = &queued[0];
        // A queued `/goal` reaches the agent as the goal's brief.
        let text = orchestration::goal::agent_text(&self.store, chat_id, next.text.clone());
        let text = self.chat_app_context(chat_id, text);
        let attachments = next.attachments.clone();
        let images = provider::load_images(&attachments)?;
        self.set_queued(chat_id, &queued, false)?;
        let input = FollowUpInput {
            message_id: &next.id,
            text: &text,
            attachments: &attachments,
            images: &images,
            merged: &[],
        };
        if !matches!(
            self.runner
                .steer(chat_id, input, chat.session_id.as_deref(), &[]),
            Delivery::NotRunning
        ) {
            self.capture_checkpoint(chat_id, &next.id);
            return Ok(());
        }
        if let Err(error) = self.runner.start_turn(
            self,
            TurnRequest {
                chat_id: chat_id.into(),
                text,
                attachments,
                images,
                resumes: false,
            },
        ) {
            self.set_queued(chat_id, &queued, true)?;
            return Err(error);
        }
        Ok(())
    }

    /// Promotes an existing queued message into the running turn.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat or the message is unknown, or when the message is no longer
    /// queued or could not be sent to the running turn.
    pub fn steer_queued(&self, chat_id: &str, message_id: &str) -> Result<(), Error> {
        let chat = self.chat(chat_id)?;
        let Some(message) = chat.messages.iter().find(|m| m.id == message_id) else {
            return Err("Message not found".into());
        };
        let subagents: Vec<String> = chat
            .messages
            .iter()
            .flat_map(|m| &m.tools)
            .filter(|c| {
                c.subagent.is_some() && c.parent.is_none() && c.status == StepStatus::Running
            })
            .map(|c| c.id.clone())
            .collect();
        let promoted =
            self.runner
                .promote_queued(chat_id, message_id, chat.session_id.as_deref(), &subagents);
        // A message queued before the running turn started is not in its queue, so it goes to
        // the agent as a new steer would.
        let steered = promoted
            || (message.queued && !self.runner.is_handed_over(message_id) && {
                let images = provider::load_images(&message.attachments)?;
                let text = self.chat_app_context(chat_id, message.text.clone());
                let input = FollowUpInput {
                    message_id,
                    text: &text,
                    attachments: &message.attachments,
                    images: &images,
                    merged: &[],
                };
                matches!(
                    self.runner
                        .steer(chat_id, input, chat.session_id.as_deref(), &subagents),
                    Delivery::Steered | Delivery::Resumed
                )
            });
        if !steered {
            return Err("That message is no longer queued or could not be sent".into());
        }
        orchestration::publish(
            self,
            chat_id,
            AgentEvent::MessageSteered {
                message_id: message_id.into(),
            },
        );
        Ok(())
    }

    /// Marks the messages `queued` as waiting in the queue or not, and tells the window.
    fn set_queued(&self, chat_id: &str, queued: &[Arc<Message>], value: bool) -> Result<(), Error> {
        self.store.update_chat(chat_id, |t| {
            t.messages
                .iter_mut()
                .filter(|m| queued.iter().any(|q| q.id == m.id))
                .for_each(|m| Arc::make_mut(m).queued = value);
        })?;
        for m in queued {
            orchestration::publish(
                self,
                chat_id,
                AgentEvent::MessageQueued {
                    message_id: m.id.clone(),
                    queued: value,
                },
            );
        }
        Ok(())
    }
}
