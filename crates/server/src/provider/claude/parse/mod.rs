//! Reads the stream-json Claude Code prints into agent events.

// Rust guideline compliant 2026-10-09

mod goal;
mod requests;
mod stream;
mod tasks;
mod todos;

#[doc(inline)]
pub use goal::{goal_outcome};
pub(crate) use requests::claude_target;
use stream::{block_text, claude_tool_use};
#[doc(inline)]
pub use stream::parse;
#[doc(inline)]
pub use tasks::{subagent_events, SHELL_TASK};
#[cfg(test)]
pub(super) use tasks::{claude_ended_status, claude_task_status};
#[doc(inline)]
pub use todos::TaskList;
