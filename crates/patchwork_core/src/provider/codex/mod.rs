//! Codex (`codex app-server`, JSON-RPC over stdio).

// Rust guideline compliant 2026-10-09

mod agent;
pub(crate) use agent::Codex;
mod input;
mod parse;
mod subagents;

#[doc(inline)]
pub use patchwork_provider_codex::{
    auth, cli, config_file, configured_model, home, models, rollout_files,
};
#[doc(inline)]
pub use input::{approval_reply, compact_args, interrupt, steer_input, turn_args, turn_input};
use input::{
    CHAT_REQUEST, GOAL_CLEAR_REQUEST, INTERRUPT_REQUEST, SKILLS_REQUEST, STEER_REQUEST, begin,
    chat_params,
};
#[doc(inline)]
pub use parse::parse;
#[doc(inline)]
pub use subagents::{background_task, subagent_events};
