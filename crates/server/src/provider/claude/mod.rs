//! Claude Code (`claude -p` with stream-json input and output).

// Rust guideline compliant 2026-10-09

pub mod cli;
mod config;
mod input;
pub mod launch;
mod parse;

#[doc(inline)]
pub use config::{home, settings_file, auth, configured_model, models};
#[doc(inline)]
pub use input::{
    turn_args, user_message, steer_message, background_task, stop_task, interrupt, approval_reply,
};
use input::PLAN_CAPTURED;
#[doc(inline)]
pub use parse::{subagent_events, SHELL_TASK, parse, goal_outcome, TaskList};
pub(crate) use parse::claude_target;

#[cfg(test)]
pub(crate) mod tests;
