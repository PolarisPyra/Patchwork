//! Claude Code (`claude -p` with stream-json input and output).

// Rust guideline compliant 2026-10-09

pub mod cli;
mod config;
mod input;
pub mod launch;
mod requests;
mod stream;
mod tasks;
mod todos;
mod writing;

#[doc(inline)]
pub use config::{home, settings_file, auth, configured_model, entrypoint, models};
pub(crate) use config::{credentials, mcp_signed_in};
#[doc(inline)]
pub use input::{turn_args, user_message, steer_message, background_task, interrupt, approval_reply};
pub(crate) use requests::claude_target;
#[doc(inline)]
pub use stream::parse;
#[doc(inline)]
pub use tasks::{subagent_events, SHELL_TASK};
#[doc(inline)]
pub use todos::TaskList;
