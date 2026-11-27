//! Claude Code (`claude -p` with stream-json input and output).

// Rust guideline compliant 2026-10-09

mod agent;
pub(crate) use agent::Claude;
mod input;
pub mod launch;
mod requests;
mod stream;
mod tasks;
mod todos;
mod writing;

#[doc(inline)]
pub use patchwork_provider_claude::{
    auth, cli, configured_model, entrypoint, home, models, settings_file,
};
pub(crate) use patchwork_provider_claude::{credentials, mcp_signed_in};
#[doc(inline)]
pub use input::{approval_reply, background_task, interrupt, steer_message, turn_args, user_message};
pub(crate) use requests::claude_target;
#[doc(inline)]
pub use stream::parse;
#[doc(inline)]
pub use tasks::{SHELL_TASK, subagent_events};
#[doc(inline)]
pub use todos::TaskList;
