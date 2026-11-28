//! The commands a message can be that the app reads itself: `/compact` and `/goal`.

// Rust guideline compliant 2026-11-27

/// `/goal`, alone or with arguments, which runs as a turn of its own.
#[must_use]
pub fn is_goal_command(text: &str) -> bool {
    text.trim_start()
        .strip_prefix("/goal")
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
}

/// `/compact`, alone or with instructions for the summary.
#[must_use]
pub fn is_compact_command(text: &str) -> bool {
    text.trim_start()
        .strip_prefix("/compact")
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
}

/// What a `/goal` message asks: `/goal` alone shows the goal; `clear` (or one of Claude's aliases
/// for it), `pause`, and `resume` act on it; anything else is a new objective.
#[derive(Debug)]
pub enum GoalCommand<'a> {
    Show,
    Clear,
    Pause,
    Resume,
    Set(&'a str),
}

/// The words after `/goal`, or None when `text` is no `/goal` message.
#[must_use]
pub fn goal_argument(text: &str) -> Option<&str> {
    is_goal_command(text).then(|| text.trim()["/goal".len()..].trim())
}

/// What the `/goal` message `text` asks, or None when it is none.
#[must_use]
pub fn goal_command(text: &str) -> Option<GoalCommand<'_>> {
    let arg = goal_argument(text)?;
    Some(match arg.to_lowercase().as_str() {
        "" => GoalCommand::Show,
        "clear" | "stop" | "off" | "reset" | "none" | "cancel" => GoalCommand::Clear,
        "pause" => GoalCommand::Pause,
        "resume" => GoalCommand::Resume,
        _ => GoalCommand::Set(arg),
    })
}
