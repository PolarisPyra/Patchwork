//! Messages sent while a turn runs: steered into it, or queued for the next turn, and taking a
//! queued one back, editing it, or sending it later or as a steer.

// Rust guideline compliant 2026-10-09

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::sync::atomic::Ordering;

use crate::system::Locked;
use crate::{provider, Error, orchestration, Core};
use crate::chats::{Attachment, Message, StepStatus};
use crate::orchestration::turn::process::write_to;
use crate::orchestration::turn::runner::watch_steer;
use crate::orchestration::turn::start::RunningTurn;
use crate::orchestration::{Runner, Delivery, TurnRequest};
use crate::events::ChatEvent;

/// A message sent during a turn, kept until the agent answers it or the turn ends.
pub(in crate::orchestration) struct FollowUp {
    /// The user message this is, so the chat can show it as queued or drop it.
    pub(in crate::orchestration) message_id: String,
    pub(in crate::orchestration) text: String,
    pub(in crate::orchestration) attachments: Vec<Attachment>,
    pub(in crate::orchestration) images: Vec<provider::Image>,
    /// The other user messages whose text this carries, when queued messages went as one.
    pub(in crate::orchestration) merged: Vec<String>,
}

impl FollowUp {
    /// The user messages this carries: its own, then the ones merged into it.
    pub(in crate::orchestration) fn ids(&self) -> impl Iterator<Item = String> {
        std::iter::once(&self.message_id)
            .chain(&self.merged)
            .cloned()
    }
}

/// A message to hold for a running turn, borrowed from the caller.
#[derive(Debug, Clone, Copy)]
pub struct FollowUpInput<'a> {
    pub message_id: &'a str,
    pub text: &'a str,
    pub attachments: &'a [Attachment],
    pub images: &'a [provider::Image],
    /// The other user messages whose text this carries.
    pub merged: &'a [String],
}

/// Steering messages written to the agent that it has not yet answered, oldest first.
pub(in crate::orchestration) type Steers = Arc<Mutex<VecDeque<FollowUp>>>;

/// Messages the agent could not take mid-turn; they start the next turn.
pub(in crate::orchestration) type Queue = Arc<Mutex<Vec<FollowUp>>>;

impl Runner {
    /// Holds a message for the next turn of the one running in `chat_id`, even when the agent
    /// could take it now. False when no turn is running there.
    pub(crate) fn enqueue(&self, chat_id: &str, input: FollowUpInput<'_>) -> bool {
        self.with_run(chat_id, |run| {
            // An idle agent is not replying, so there is nothing to wait for — unless Antigravity
            // is still working on an unprompted reply of its own.
            let Some(turn) = run
                .turn
                .as_ref()
                .filter(|t| !t.idle.load(Ordering::SeqCst) || t.alone.load(Ordering::SeqCst))
            else {
                return false;
            };
            let mut queue = turn.queue.locked();
            run.handed_over.insert(input.message_id.into());
            queue.push(FollowUp {
                message_id: input.message_id.into(),
                text: input.text.into(),
                attachments: input.attachments.to_vec(),
                images: input.images.to_vec(),
                merged: input.merged.to_vec(),
            });
            true
        })
    }

    /// Replaces the payload of a queued message in the active turn. False if it has already
    /// been promoted, withdrawn, or the turn ended.
    pub fn edit_queued(&self, chat_id: &str, input: FollowUpInput<'_>) -> bool {
        self.with_turn(chat_id, |turn| {
            let mut queue = turn.queue.locked();
            let Some(follow_up) = queue.iter_mut().find(|f| f.message_id == input.message_id)
            else {
                return false;
            };
            follow_up.text = input.text.into();
            follow_up.attachments = input.attachments.to_vec();
            follow_up.images = input.images.to_vec();
            true
        })
        .unwrap_or(false)
    }

    /// Whether the message is waiting in the queue of the turn running in `chat_id`.
    pub fn is_queued(&self, chat_id: &str, message_id: &str) -> bool {
        self.with_turn(chat_id, |turn| {
            turn.queue
                .locked()
                .iter()
                .any(|f| f.message_id == message_id)
        })
        .unwrap_or(false)
    }

    /// Takes a queued message back so it is not sent. False if the agent has already been handed
    /// it. A message this process never handed over was queued before a restart, so nothing
    /// holds it and it counts as taken back.
    pub fn withdraw(&self, chat_id: &str, message_id: &str) -> bool {
        self.with_run(chat_id, |run| {
            if let Some(turn) = &run.turn {
                let mut queue = turn.queue.locked();
                if let Some(index) = queue.iter().position(|f| f.message_id == message_id) {
                    queue.remove(index);
                    drop(queue);
                    run.handed_over.remove(message_id);
                    return true;
                }
            }
            !run.handed_over.contains(message_id)
        })
    }

    /// Whether a running turn was given the message and has not handed it back: it waits in the
    /// turn's queue, or the agent has it.
    pub fn is_handed_over(&self, chat_id: &str, message_id: &str) -> bool {
        self.with_run(chat_id, |run| run.handed_over.contains(message_id))
    }

    /// Promotes an existing queued message into the active turn without removing and recreating
    /// it. Returns false if the message is no longer queued or cannot be written to the agent.
    pub fn promote_queued(
        &self,
        chat_id: &str,
        message_id: &str,
        session_id: Option<&str>,
        subagents: &[String],
    ) -> bool {
        self.background_subagents(chat_id, subagents);
        self.with_turn(chat_id, |turn| promote(turn, message_id, session_id))
            .unwrap_or(false)
    }
}

/// Writes the queued message `message_id` to `turn`'s agent, as `promote_queued` does.
fn promote(turn: &RunningTurn, message_id: &str, session_id: Option<&str>) -> bool {
    let mut queue = turn.queue.locked();
    let Some(index) = queue.iter().position(|f| f.message_id == message_id) else {
        return false;
    };
    if turn.cancelled.load(Ordering::SeqCst) {
        return false;
    }
    let follow_up = queue.remove(index);
    let turn_id = turn.native.locked().clone();
    let prompt = provider::Prompt {
        text: &follow_up.text,
        attachments: &follow_up.attachments,
        images: &follow_up.images,
    };
    let mut stdin = turn.stdin.locked();
    if turn.idle.load(Ordering::SeqCst) {
        let line = provider::resume_input(turn.agent, prompt, session_id, turn_id.as_deref()).map(
            |line| {
                let mode = *turn.mode.locked();
                turn.settings
                    .locked()
                    .first_line(turn.agent, mode, session_id, line)
            },
        );
        if !turn.stale.load(Ordering::SeqCst)
            && line.is_some_and(|line| write_to(&mut stdin, &line))
        {
            turn.idle.store(false, Ordering::SeqCst);
            turn.steers.locked().push_back(follow_up);
            turn.wake.reply_due();
            return true;
        }
        queue.insert(index, follow_up);
        return false;
    }
    let line = provider::steer_input(turn.agent, prompt, session_id, turn_id.as_deref());
    // As in `steer`: during Patchwork's compaction, or when Antigravity is already cancelling
    // for an earlier steer, wait instead of writing another cancel.
    let already_cancelling = turn.agent.speaks_acp() && turn.cancelling.load(Ordering::SeqCst);
    let waits = turn.compacting.load(Ordering::SeqCst) || already_cancelling;
    if waits {
        turn.steers.locked().push_back(follow_up);
        true
    } else if let Some(line) = line.filter(|line| write_to(&mut stdin, line)) {
        let id = follow_up.message_id.clone();
        turn.steers.locked().push_back(follow_up);
        watch_steer(turn, &id, line);
        true
    } else {
        queue.insert(index, follow_up);
        false
    }
}

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
            ChatEvent::MessageRemoved {
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
            && self.runner.is_handed_over(chat_id, message_id)
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
        orchestration::publish(self, chat_id, ChatEvent::MessageEdited { message: edited });
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
    /// queued or could not be sent to the running turn, or while Antigravity compacts the chat.
    pub(crate) fn steer_queued(&self, chat_id: &str, message_id: &str) -> Result<(), Error> {
        // A steer would stop the compaction, and the message is sent once it ends anyway.
        if self.compacting(chat_id) {
            return Err(
                "Antigravity is compacting this chat; the message goes once it ends".into(),
            );
        }
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
            || (message.queued && !self.runner.is_handed_over(chat_id, message_id) && {
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
            ChatEvent::MessageSteered {
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
                ChatEvent::MessageQueued {
                    message_id: m.id.clone(),
                    queued: value,
                },
            );
        }
        Ok(())
    }
}
