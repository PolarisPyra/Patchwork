//! Resuming a turn the app's closing cut off: at the next launch by itself, once, and after that
//! when the user asks. A turn an error ended resumes the same way, only when asked.
//!
//! A reply is interrupted when Patchwork quit, restarted for an update, or crashed while it ran.
//! The agent's own session still holds everything up to that point, so the turn starts again in
//! that session with a note saying what happened, and its reply follows the one that was cut off.
//! A goal the turn was pursuing, which the closing paused, is pursued again with it.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use crate::events::AgentEvent;
use crate::orchestration::turn::start::TurnRequest;
use crate::orchestration::{self, goal};
use crate::settings::ResumeAtLaunch;
use crate::chats::{Goal, GoalStatus, MessageStatus, Role, Chat};
use crate::{Error, Core};

/// What the agent is told in place of a message from the user, after `why` its reply ended.
///
/// It says what was lost, because the agent's session does not: the session ends mid-step, and
/// the commands and subagents of that step ended with the agent's process.
fn note(cut_off: MessageStatus) -> String {
    let why = if cut_off == MessageStatus::Error {
        "Your last reply ended with an error before it was finished"
    } else {
        "Patchwork closed while you were working, so your last reply was cut off"
    };
    format!(
        "{why}. Any command, subagent, or background task that was running then has stopped, \
         and its result is lost. Check what was already done, then carry on with the task from \
         where you left off. Do not start it over."
    )
}

/// Whether the cut-off reply was itself carrying on from a cut-off reply. Such a turn is not
/// resumed without being asked: had it brought the app down, resuming it at every launch would
/// bring the app down at every launch.
fn interrupted_twice(chat: &Chat) -> bool {
    chat.messages
        .iter()
        .rev()
        .filter(|m| !m.queued)
        .nth(1)
        .is_some_and(|m| m.role == Role::Assistant && m.status == MessageStatus::Interrupted)
}

impl Core {
    /// The chats whose cut-off turn can be resumed without asking about each.
    ///
    /// Left for the user to resume one by one: an archived chat, and a turn already resumed
    /// once and cut off again. A Patchwork running beside another has none, since the turns may
    /// be live in that one.
    fn waiting(&self) -> Vec<String> {
        if !self.store.owns_data() {
            return Vec::new();
        }
        self.store.with_chats(|chats| {
            chats
                .iter()
                .filter(|t| t.archived_at.is_none() && t.parent.is_none())
                .filter(|t| t.interrupted_reply().is_some() && !interrupted_twice(t))
                .map(|t| t.id.clone())
                .collect()
        })
    }

    /// Does what Settings says a launch does with the turns the last session cut off, and
    /// returns how many wait for the user to say so. Called once, after the app has opened.
    ///
    /// Set to always, they are resumed and none wait. Set to ask, they all wait, for
    /// `resume_interrupted`. Set to never, each is left to its own Resume button.
    pub fn resume_at_launch(self: &Arc<Self>) -> usize {
        match self.store.settings().resume_at_launch {
            ResumeAtLaunch::Always => {
                self.resume_interrupted();
                0
            }
            ResumeAtLaunch::Ask => self.waiting().len(),
            ResumeAtLaunch::Never => 0,
        }
    }

    /// Resumes every turn the last session cut off, and returns how many it started. A turn
    /// that cannot start (its project's folder is gone, say) is left for the user to resume.
    pub fn resume_interrupted(self: &Arc<Self>) -> usize {
        self.waiting()
            .iter()
            .filter(|id| match self.resume_turn(id) {
                Ok(()) => true,
                Err(e) => {
                    log::warn!(
                        chat = id.as_str(), error:% = e;
                        "could not resume the turn of {{chat}}: {{error}}"
                    );
                    false
                }
            })
            .count()
    }

    /// Resumes the turn whose reply the app's closing cut off or an error ended, in the agent's
    /// own session.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat is unknown, when its last reply neither was cut off nor
    /// failed, when a turn is already running, or when the turn cannot be started.
    pub fn resume_turn(self: &Arc<Self>, chat_id: &str) -> Result<(), Error> {
        let chat = self.chat(chat_id)?;
        let Some(reply) = chat.resumable_reply() else {
            return Err("This reply ended as it should, so there is nothing to resume".into());
        };
        let cut_off = reply.status;
        // A cut-off `/compact` is asked again: told to carry on, the agent would go on with the
        // task instead of summarizing.
        let compact = chat
            .messages
            .iter()
            .rev()
            .skip_while(|m| m.id != reply.id)
            .find(|m| m.role == Role::User && !m.queued)
            .filter(|m| crate::provider::is_compact_command(&m.text))
            .map(|m| m.text.trim().to_string());
        if self.runner.is_running(chat_id) {
            return Err("This chat already has a reply running".into());
        }
        // A goal the turn was pursuing goes on with it.
        let goal = chat
            .goal
            .filter(|g| goal::take_cut_off(&self.store, chat_id, g));
        // The goal is active before the turn starts, so the turn's end carries it on.
        if let Some(goal) = &goal {
            set_goal_status(self, chat_id, goal, GoalStatus::Active);
        }
        let started = self.runner.start_turn(
            self,
            TurnRequest {
                chat_id: chat_id.into(),
                text: compact.unwrap_or_else(|| note(cut_off)),
                attachments: Vec::new(),
                images: Vec::new(),
                resumes: true,
            },
        );
        if let Err(e) = started {
            // The turn did not start, so the goal is still the one a later resume takes up.
            if let Some(goal) = &goal {
                set_goal_status(self, chat_id, goal, GoalStatus::Paused);
                goal::note_cut_off(&self.store, chat_id, goal, true);
            }
            return Err(e);
        }
        Ok(())
    }
}

/// Publishes `goal` with `status`.
fn set_goal_status(core: &Arc<Core>, chat_id: &str, goal: &Goal, status: GoalStatus) {
    orchestration::publish(
        core,
        chat_id,
        AgentEvent::GoalUpdated {
            goal: Some(goal.clone().with_status(status, crate::system::now_ms())),
        },
    );
}
