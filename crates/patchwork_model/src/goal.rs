//! Telling the messages the app sends on its own from work for the agent.

// Rust guideline compliant 2026-11-27

use crate::provider::{GoalCommand, goal_command, is_compact_command};

/// How the message that carries a goal on starts.
pub const CONTINUE: &str = "Continue working toward the goal: ";

/// Whether `text` is the message Patchwork sent to carry a goal on, which the chat does not show.
#[must_use]
pub fn is_goal_continuation(text: &str) -> bool {
    text.starts_with(CONTINUE)
}

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
    if is_compact_command(text) {
        Some(Upkeep::Compact)
    } else if matches!(
        goal_command(text),
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
