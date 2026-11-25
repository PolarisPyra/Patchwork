//! Codex (`codex app-server`, JSON-RPC over stdio).

// Rust guideline compliant 2026-10-09

pub mod cli;
mod config;
mod input;
mod parse;
mod subagents;

#[doc(inline)]
pub use config::{home, config_file, auth, configured_model, models, rollout_files};
#[doc(inline)]
pub use input::{turn_args, compact_args, turn_input, steer_input, interrupt, approval_reply};
use input::{
    CHAT_REQUEST, chat_params, SKILLS_REQUEST, begin, STEER_REQUEST, INTERRUPT_REQUEST,
    GOAL_CLEAR_REQUEST,
};
#[doc(inline)]
pub use parse::parse;
#[doc(inline)]
pub use subagents::{background_task, subagent_events};
