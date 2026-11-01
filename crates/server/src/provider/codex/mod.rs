//! Codex (`codex app-server`, JSON-RPC over stdio).

pub mod cli;
mod config;
mod input;
mod parse;

pub use config::{home, auth, configured_model, models, rollout_files};
pub use input::{
    turn_args, agents_args, compact_args, turn_input, goal_pause, steer_input, interrupt,
    approval_reply,
};
use input::{
    THREAD_REQUEST, thread_params, SKILLS_REQUEST, TURN_REQUEST, begin, GOAL_REQUEST, GOAL_SETUP,
    goal_of, goal_reply, STEER_REQUEST, INTERRUPT_REQUEST,
};
pub use parse::{subagent_events, background_task, parse};

#[cfg(test)]
pub(crate) mod tests;
