//! Reads the stream-json Claude Code prints into agent events.

mod goal;
mod requests;
mod stream;
mod tasks;
mod todos;

pub use goal::{goal_outcome};
pub(crate) use requests::claude_target;
pub use stream::*;
pub use tasks::{subagent_events, SHELL_TASK};
#[cfg(test)]
pub(super) use tasks::{claude_ended_status, claude_task_status};
pub use todos::TaskList;
