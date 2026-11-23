//! Messages sent while a turn runs: steered into it, or queued for the next turn.

// Rust guideline compliant 2026-10-09

use crate::system::Locked;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::Mutex;

use crate::provider;
use crate::chats::Attachment;
use crate::orchestration::turn::process::write_to;
use crate::orchestration::turn::runner::watch_steer;
use crate::orchestration::Runner;

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
        let turns = self.turns.locked();
        // An idle agent is not replying, so there is nothing to wait for — unless Antigravity is
        // still working on an unprompted reply of its own.
        let Some(turn) = turns
            .get(chat_id)
            .filter(|t| !t.idle.load(Ordering::SeqCst) || t.alone.load(Ordering::SeqCst))
        else {
            return false;
        };
        let mut queue = turn.queue.locked();
        self.handed_over.locked().insert(input.message_id.into());
        queue.push(FollowUp {
            message_id: input.message_id.into(),
            text: input.text.into(),
            attachments: input.attachments.to_vec(),
            images: input.images.to_vec(),
            merged: input.merged.to_vec(),
        });
        true
    }

    /// Replaces the payload of a queued message in the active turn. False if it has already
    /// been promoted, withdrawn, or the turn ended.
    pub fn edit_queued(&self, chat_id: &str, input: FollowUpInput<'_>) -> bool {
        let turns = self.turns.locked();
        let Some(turn) = turns.get(chat_id) else {
            return false;
        };
        let mut queue = turn.queue.locked();
        let Some(follow_up) = queue.iter_mut().find(|f| f.message_id == input.message_id) else {
            return false;
        };
        follow_up.text = input.text.into();
        follow_up.attachments = input.attachments.to_vec();
        follow_up.images = input.images.to_vec();
        true
    }

    /// Whether the message is waiting in the queue of the turn running in `chat_id`.
    pub fn is_queued(&self, chat_id: &str, message_id: &str) -> bool {
        let turns = self.turns.locked();
        turns.get(chat_id).is_some_and(|turn| {
            turn.queue
                .locked()
                .iter()
                .any(|f| f.message_id == message_id)
        })
    }

    /// Takes a queued message back so it is not sent. False if the agent has already been handed
    /// it. A message this process never handed over was queued before a restart, so nothing
    /// holds it and it counts as taken back.
    pub fn withdraw(&self, chat_id: &str, message_id: &str) -> bool {
        let turns = self.turns.locked();
        if let Some(turn) = turns.get(chat_id) {
            let mut queue = turn.queue.locked();
            if let Some(index) = queue.iter().position(|f| f.message_id == message_id) {
                queue.remove(index);
                self.handed_over.locked().remove(message_id);
                return true;
            }
        }
        !self.handed_over.locked().contains(message_id)
    }

    /// Whether a running turn was given the message and has not handed it back: it waits in the
    /// turn's queue, or the agent has it.
    pub fn is_handed_over(&self, message_id: &str) -> bool {
        self.handed_over.locked().contains(message_id)
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
        let turns = self.turns.locked();
        let Some(turn) = turns.get(chat_id) else {
            return false;
        };
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
            let line = provider::resume_input(turn.agent, prompt, session_id, turn_id.as_deref())
                .map(|line| {
                    let mode = *turn.mode.locked();
                    turn.settings
                        .locked()
                        .first_line(turn.agent, mode, session_id, line)
                });
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
        let already_cancelling = turn.agent == provider::AgentKind::Antigravity
            && turn.cancelling.load(Ordering::SeqCst);
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
}
