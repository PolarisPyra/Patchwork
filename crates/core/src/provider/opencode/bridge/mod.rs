//! Bridge that runs an OpenCode 2.x turn through a private `opencode serve`.
//!
//! Runs one OpenCode 2.x turn through a private `opencode serve`, so the turn can ask for
//! approvals, ask questions, and take steering messages. Patchwork starts this bridge as the
//! turn's process: the first stdin line is the turn, later lines are replies and steering
//! messages, and stdout carries the session's server events as JSON lines, plus `bridge.*`
//! lines of its own. The bridge and its server end with the turn.

// Rust guideline compliant 2026-10-09

mod turn;
mod server;

pub use turn::{BRIDGE_ARG, Request, SlashCommand, run};
