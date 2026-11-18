//! The `/goal` commands Patchwork answers itself, which never reach the agent: pausing and
//! clearing the goal.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::sync::Arc;

use super::state::signature;
use crate::Core;
use crate::orchestration;
use crate::events::AgentEvent;
use crate::provider::GoalCommand;
use crate::system::now_ms;
use crate::threads::GoalStatus;

/// The messages the app also sends on its own rather than as work for the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Upkeep {
    /// `/compact`: the conversation is summarized.
    Compact,
    /// A `/goal` command Patchwork answers itself: `/goal` alone, a clear, or a pause.
    Goal,
}

/// What upkeep `text` is, or None when it is work for the agent.
#[must_use]
pub fn upkeep(text: &str) -> Option<Upkeep> {
    let text = text.trim();
    if text == "/compact" {
        Some(Upkeep::Compact)
    } else if matches!(
        crate::provider::goal_command(text),
        Some(GoalCommand::Show | GoalCommand::Clear | GoalCommand::Pause)
    ) {
        Some(Upkeep::Goal)
    } else {
        None
    }
}

/// Whether `text` is upkeep the app also sends on its own rather than work for the agent.
#[must_use]
pub fn is_upkeep(text: &str) -> bool {
    upkeep(text).is_some()
}

impl Core {
    /// Clears the goal. A reply still running finishes, and nothing carries the goal on after it.
    ///
    /// # Errors
    ///
    /// Returns an error when the goal's dismissal cannot be saved.
    pub fn stop_goal(self: &Arc<Self>, thread_id: &str) -> Result<(), Error> {
        let Some(goal) = self.store.thread(thread_id).and_then(|t| t.goal) else {
            return Ok(());
        };
        self.store.dismiss_goal(thread_id, &signature(&goal))?;
        orchestration::publish(self, thread_id, AgentEvent::GoalUpdated { goal: None });
        Ok(())
    }

    /// Pauses the goal. A reply still running finishes, and nothing carries the goal on after it
    /// until it is resumed.
    pub(crate) fn pause_goal(self: &Arc<Self>, thread_id: &str) {
        let Some(goal) = self
            .store
            .thread(thread_id)
            .and_then(|t| t.goal)
            .filter(|g| g.status == GoalStatus::Active)
        else {
            return;
        };
        orchestration::publish(
            self,
            thread_id,
            AgentEvent::GoalUpdated {
                goal: Some(goal.with_status(GoalStatus::Paused, now_ms())),
            },
        );
    }
}
