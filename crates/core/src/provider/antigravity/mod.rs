//! Antigravity (`agy acp`, Agent Client Protocol JSON-RPC over stdio).

// Rust guideline compliant 2026-11-23

mod calls;
mod children;
mod cli;
mod dispatch;
mod edits;
mod errors;
mod input;
mod login;
mod models;
mod permission_prompts;
mod probe;
mod questions;
mod replies;
mod session_updates;
mod swarm;
mod tool_fields;

#[doc(inline)]
pub use calls::ToolCalls;
#[doc(inline)]
pub use children::{Transcript, child_transcripts};
#[doc(inline)]
pub use cli::{antigravity_bin, command, home, info};
#[doc(inline)]
pub use models::{models, parse_acp_models, parse_models};
#[doc(inline)]
pub use probe::{auth, configured_model, known_defaults};
pub(crate) use probe::tier;
#[doc(inline)]
pub use input::{
    Wanted, approval_reply, configure, interrupt, prompt_input, turn_args, turn_env, turn_input,
};
#[doc(inline)]
pub use login::{LoginSession, sign_out, start_login};
#[doc(inline)]
pub use dispatch::{background_task, is_work, parse};
#[doc(inline)]
pub use questions::BY_MESSAGE;
#[doc(inline)]
pub use session_updates::CONTEXT_REQUEST;
#[doc(inline)]
pub use swarm::{SWARM_POLL, SWARM_TASK_PREFIX, Swarm};
