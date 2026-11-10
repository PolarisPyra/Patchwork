//! The messages queued behind a running turn: taking one back, editing it, sending it later or
//! as a steer.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::sync::Arc;

use crate::orchestration::{Delivery, FollowUpInput, TurnRequest};

use crate::events::AgentEvent;
use crate::threads::{StepStatus, Attachment};
use crate::{orchestration, provider};
use crate::Server;

impl Server {
    /// Takes back a message sent mid-turn that has not started yet, so it can be edited.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat is unknown, when the message was already sent, or when the
    /// removal cannot be saved.
    pub fn remove_queued(&self, thread_id: &str, message_id: &str) -> Result<(), Error> {
        let thread = self.thread(thread_id)?;
        let Some(message) = thread.messages.iter().find(|m| m.id == message_id) else {
            // Makes startup cleanup safe to retry if an app refresh overlaps another cleanup.
            return Ok(());
        };
        if !message.queued {
            return Err("That message has already been sent".into());
        }
        let running = self.runner.is_running(thread_id);
        let withdrawn = self.runner.withdraw(thread_id, message_id);
        if running && !withdrawn {
            return Err("That message has already been sent".into());
        }
        orchestration::publish(
            self,
            thread_id,
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
        thread_id: &str,
        message_id: &str,
        text: String,
        attachments: Vec<Attachment>,
    ) -> Result<(), Error> {
        let images = provider::load_images(&attachments)?;
        let thread = self.thread(thread_id)?;
        let original = thread
            .messages
            .iter()
            .find(|m| m.id == message_id && m.queued)
            .ok_or("That message is no longer queued")?;
        // A message the running turn does not hold was queued before it started: after a Stop,
        // or before a restart. It waits in the thread alone, so the edit below is all of it.
        if self.runner.is_running(thread_id)
            && !self.runner.edit_queued(
                thread_id,
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
        let mut edited = original.clone();
        edited.text = text;
        edited.attachments = attachments;
        self.store.update_thread(thread_id, |t| {
            if let Some(m) = t
                .messages
                .iter_mut()
                .find(|m| m.id == message_id && m.queued)
            {
                m.text.clone_from(&edited.text);
                m.attachments.clone_from(&edited.attachments);
            }
        })?;
        orchestration::publish(
            self,
            thread_id,
            AgentEvent::MessageEdited { message: edited },
        );
        Ok(())
    }

    /// Sends the thread's queued messages once its turn has ended, in the background.
    pub(crate) fn send_queued_later(self: &Arc<Self>, thread_id: &str) {
        let server = Arc::clone(self);
        let thread_id = thread_id.to_string();
        std::thread::spawn(move || {
            let queued = server
                .store
                .thread(&thread_id)
                .is_some_and(|t| t.messages.iter().any(|m| m.queued));
            if queued && let Err(e) = server.resume_queued(&thread_id) {
                log::warn!(error:% = e; "failed to send queued messages: {{error}}");
            }
        });
    }

    /// Sends persisted queued messages in their original order, as one message: to the idle
    /// agent when one is up, as a normal message would go, otherwise in a new turn.
    ///
    /// # Errors
    ///
    /// Returns an error when a turn is already running, when the chat is unknown or has no queued
    /// messages, when an attachment image cannot be read, or when the queue cannot be saved or the
    /// turn cannot be started.
    pub fn resume_queued(self: &Arc<Self>, thread_id: &str) -> Result<(), Error> {
        if self.runner.is_running(thread_id) {
            return Err("This turn is already running".into());
        }
        let thread = self.thread(thread_id)?;
        let queued: Vec<_> = thread.messages.into_iter().filter(|m| m.queued).collect();
        if queued.is_empty() {
            return Err("There are no queued messages to resume".into());
        }
        let text = queued
            .iter()
            .map(|m| m.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let attachments: Vec<_> = queued.iter().flat_map(|m| m.attachments.clone()).collect();
        let images = provider::load_images(&attachments)?;
        self.store.update_thread(thread_id, |t| {
            t.messages
                .iter_mut()
                .filter(|m| queued.iter().any(|q| q.id == m.id))
                .for_each(|m| m.queued = false);
        })?;
        for m in &queued {
            orchestration::publish(
                self,
                thread_id,
                AgentEvent::MessageQueued {
                    message_id: m.id.clone(),
                    queued: false,
                },
            );
        }
        let first = &queued[0].id;
        // Held with the first if the agent never answers, so none of them is left as sent.
        let merged: Vec<String> = queued[1..].iter().map(|m| m.id.clone()).collect();
        let input = FollowUpInput {
            message_id: first,
            text: &text,
            attachments: &attachments,
            images: &images,
            merged: &merged,
        };
        if !matches!(
            self.runner
                .steer(thread_id, input, thread.session_id.as_deref(), &[]),
            Delivery::NotRunning
        ) {
            // The reply answers the newest of the merged messages, which is the one a checkpoint
            // is looked up under.
            if let Some(last) = queued.last() {
                self.capture_checkpoint(thread_id, &last.id);
            }
            return Ok(());
        }
        if let Err(error) = self.runner.start_turn(
            self,
            TurnRequest {
                thread_id: thread_id.into(),
                text,
                attachments,
                images,
                resumes: false,
            },
        ) {
            self.store.update_thread(thread_id, |t| {
                t.messages
                    .iter_mut()
                    .filter(|m| queued.iter().any(|q| q.id == m.id))
                    .for_each(|m| m.queued = true);
            })?;
            for m in &queued {
                orchestration::publish(
                    self,
                    thread_id,
                    AgentEvent::MessageQueued {
                        message_id: m.id.clone(),
                        queued: true,
                    },
                );
            }
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
    pub fn steer_queued(&self, thread_id: &str, message_id: &str) -> Result<(), Error> {
        let thread = self.thread(thread_id)?;
        let Some(message) = thread.messages.iter().find(|m| m.id == message_id) else {
            return Err("Message not found".into());
        };
        let subagents: Vec<String> = thread
            .messages
            .iter()
            .flat_map(|m| &m.tools)
            .filter(|c| {
                c.subagent.is_some() && c.parent.is_none() && c.status == StepStatus::Running
            })
            .map(|c| c.id.clone())
            .collect();
        let promoted = self.runner.promote_queued(
            thread_id,
            message_id,
            thread.session_id.as_deref(),
            &subagents,
        );
        // A message queued before the running turn started is not in its queue, so it goes to
        // the agent as a new steer would.
        let steered = promoted
            || (message.queued && !self.runner.is_handed_over(message_id) && {
                let images = provider::load_images(&message.attachments)?;
                let input = FollowUpInput {
                    message_id,
                    text: &message.text,
                    attachments: &message.attachments,
                    images: &images,
                    merged: &[],
                };
                matches!(
                    self.runner
                        .steer(thread_id, input, thread.session_id.as_deref(), &subagents),
                    Delivery::Steered | Delivery::Resumed
                )
            });
        if !steered {
            return Err("That message is no longer queued or could not be sent".into());
        }
        orchestration::publish(
            self,
            thread_id,
            AgentEvent::MessageSteered {
                message_id: message_id.into(),
            },
        );
        Ok(())
    }
}
