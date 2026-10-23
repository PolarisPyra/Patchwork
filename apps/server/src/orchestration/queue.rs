//! Messages sent while a turn runs: steered into it, or queued for the next turn.
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::sync::PoisonError;

use crate::provider;
use crate::types::Attachment;
use super::process::write_to;
use super::Runner;


/// A message sent during a turn, kept until the agent answers it or the turn ends.
pub(super) struct FollowUp {
    /// The user message this is, so the thread can show it as queued or drop it.
    pub(super) message_id: String,
    pub(super) text: String,
    pub(super) attachments: Vec<Attachment>,
    pub(super) images: Vec<provider::Image>,
}

/// A message to hold for a running turn, borrowed from the caller.
pub struct FollowUpInput<'a> {
    pub message_id: &'a str,
    pub text: &'a str,
    pub attachments: &'a [Attachment],
    pub images: &'a [provider::Image],
}

/// Steering messages written to the agent that it has not yet answered, oldest first.
pub(super) type Steers = Arc<Mutex<VecDeque<FollowUp>>>;

/// Messages the agent could not take mid-turn; they start the next turn.
pub(super) type Queue = Arc<Mutex<Vec<FollowUp>>>;

impl Runner {
    /// Holds a message for the next turn of the one running in `thread_id`, even when the agent
    /// could take it now. False when no turn is running there.
    pub fn enqueue(&self, thread_id: &str, input: FollowUpInput<'_>) -> Result<bool, String> {
        let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(turn) = turns.get(thread_id) else {
            return Ok(false);
        };
        let mut queue = turn.queue.lock().unwrap_or_else(PoisonError::into_inner);
        if !queue.is_empty() {
            return Err("Only one message can be queued at a time. Edit or remove the queued message first.".into());
        }
        self.handed_over
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(input.message_id.into());
        queue.push(FollowUp {
            message_id: input.message_id.into(),
            text: input.text.into(),
            attachments: input.attachments.to_vec(),
            images: input.images.to_vec(),
        });
        Ok(true)
    }

    /// Replaces the payload of a queued message in the active turn. False if it has already
    /// been promoted, withdrawn, or the turn ended.
    pub fn edit_queued(&self, thread_id: &str, input: FollowUpInput<'_>) -> bool {
        let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(turn) = turns.get(thread_id) else { return false };
        let mut queue = turn.queue.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(follow_up) = queue.iter_mut().find(|f| f.message_id == input.message_id) else {
            return false;
        };
        follow_up.text = input.text.into();
        follow_up.attachments = input.attachments.to_vec();
        follow_up.images = input.images.to_vec();
        true
    }

    /// Whether the message is waiting in the queue of the turn running in `thread_id`.
    pub fn is_queued(&self, thread_id: &str, message_id: &str) -> bool {
        let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        turns.get(thread_id).is_some_and(|turn| {
            turn.queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .any(|f| f.message_id == message_id)
        })
    }

    /// Takes a queued message back so it is not sent. False if the agent has already been handed
    /// it. A message this process never handed over was queued before a restart, so nothing
    /// holds it and it counts as taken back.
    pub fn withdraw(&self, thread_id: &str, message_id: &str) -> bool {
        let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(turn) = turns.get(thread_id) {
            let mut queue = turn.queue.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(index) = queue.iter().position(|f| f.message_id == message_id) {
                queue.remove(index);
                self.handed_over
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(message_id);
                return true;
            }
        }
        !self
            .handed_over
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(message_id)
    }

    /// Promotes an existing queued message into the active turn without removing and recreating
    /// it. Returns false if the message is no longer queued or cannot be written to the agent.
    pub fn promote_queued(
        &self,
        thread_id: &str,
        message_id: &str,
        session_id: Option<&str>,
        subagents: &[String],
    ) -> bool {
        self.background_subagents(thread_id, subagents);
        let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(turn) = turns.get(thread_id) else { return false };
        let mut queue = turn.queue.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(index) = queue.iter().position(|f| f.message_id == message_id) else { return false };
        if turn.cancelled.load(Ordering::SeqCst) { return false; }
        let follow_up = queue.remove(index);
        let turn_id = turn.native_turn.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let prompt = provider::Prompt {
            text: &follow_up.text,
            attachments: &follow_up.attachments,
            images: &follow_up.images,
        };
        let line = provider::steer_input(turn.agent, prompt, session_id, turn_id.as_deref());
        let mut stdin = turn.stdin.lock().unwrap_or_else(PoisonError::into_inner);
        if line.as_deref().is_some_and(|line| write_to(&mut stdin, line)) {
            turn.steers.lock().unwrap_or_else(PoisonError::into_inner).push_back(follow_up);
            true
        } else {
            queue.insert(index, follow_up);
            false
        }
    }
}
