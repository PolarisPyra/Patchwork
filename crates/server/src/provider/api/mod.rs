//! API providers as an agent: a turn runs Patchwork's own harness (`crate::harness`) against the
//! provider of the task's model, and this maps the harness's `api.*` lines.

// Rust guideline compliant 2026-10-09

pub mod config;
pub mod servers;

mod turn;

#[doc(inline)]
pub use turn::{
    approval_reply, auth, bridge_request, compact_window_line, mcp_reply, parse, steer_line,
    turn_args,
};
