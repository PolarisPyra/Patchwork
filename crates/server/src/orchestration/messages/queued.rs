//! The messages queued behind a running turn: taking one back, editing it, sending it later or
//! as a steer.

use std::sync::Arc;

use crate::orchestration::{Delivery, FollowUpInput, TurnRequest};

use crate::orchestration::AgentEvent;
use crate::threads::{StepStatus, Attachment};
use crate::{orchestration, provider};
use crate::Server;

impl Server {
    /// Takes back a message sent mid-turn that has not started yet, so it can be edited.
    pub fn remove_queued(&self, thread_id: &str, message_id: &str) -> Result<(), String> {
        let Some(thread) = self.store.thread(thread_id) else {
            return Err("Thread not found".into());
        };
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
    pub fn edit_queued(
        &self,
        thread_id: &str,
        message_id: &str,
        text: String,
        attachments: Vec<Attachment>,
    ) -> Result<(), String> {
        let images = provider::load_images(&attachments)?;
        let thread = self.store.thread(thread_id).ok_or("Thread not found")?;
        let original = thread
            .messages
            .iter()
            .find(|m| m.id == message_id && m.queued)
            .ok_or("That message is no longer queued")?;
        if self.runner.is_running(thread_id)
            && !self.runner.edit_queued(
                thread_id,
                FollowUpInput {
                    message_id,
                    text: &text,
                    attachments: &attachments,
                    images: &images,
                },
            )
        {
            return Err("That message has already been promoted".into());
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
                m.text = edited.text.clone();
                m.attachments = edited.attachments.clone();
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
                log::warn!("failed to send queued messages: {e}");
            }
        });
    }

    /// Sends persisted queued messages in their original order, as one message: to the idle
    /// agent when one is up, as a normal message would go, otherwise in a new turn.
    pub fn resume_queued(self: &Arc<Self>, thread_id: &str) -> Result<(), String> {
        if self.runner.is_running(thread_id) {
            return Err("This turn is already running".into());
        }
        let thread = self.store.thread(thread_id).ok_or("Thread not found")?;
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
        queued.iter().for_each(|m| {
            orchestration::publish(
                self,
                thread_id,
                AgentEvent::MessageQueued {
                    message_id: m.id.clone(),
                    queued: false,
                },
            )
        });
        let first = &queued[0].id;
        let input = FollowUpInput {
            message_id: first,
            text: &text,
            attachments: &attachments,
            images: &images,
        };
        if !matches!(
            self.runner
                .steer(thread_id, input, thread.session_id.as_deref(), &[]),
            Delivery::NotRunning
        ) {
            self.capture_checkpoint(thread_id, first);
            return Ok(());
        }
        if let Err(error) = self.runner.start_turn(
            self,
            TurnRequest {
                thread_id: thread_id.into(),
                text,
                attachments,
                images,
            },
        ) {
            self.store.update_thread(thread_id, |t| {
                t.messages
                    .iter_mut()
                    .filter(|m| queued.iter().any(|q| q.id == m.id))
                    .for_each(|m| m.queued = true);
            })?;
            queued.iter().for_each(|m| {
                orchestration::publish(
                    self,
                    thread_id,
                    AgentEvent::MessageQueued {
                        message_id: m.id.clone(),
                        queued: true,
                    },
                )
            });
            return Err(error);
        }
        Ok(())
    }

    /// Promotes an existing queued message into the running turn.
    pub fn steer_queued(&self, thread_id: &str, message_id: &str) -> Result<(), String> {
        let thread = self.store.thread(thread_id).ok_or("Thread not found")?;
        if !thread.messages.iter().any(|m| m.id == message_id) {
            return Err("Message not found".into());
        }
        let subagents: Vec<String> = thread
            .messages
            .iter()
            .flat_map(|m| &m.tools)
            .filter(|c| {
                c.subagent.is_some() && c.parent.is_none() && c.status == StepStatus::Running
            })
            .map(|c| c.id.clone())
            .collect();
        if !self.runner.promote_queued(
            thread_id,
            message_id,
            thread.session_id.as_deref(),
            &subagents,
        ) {
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
