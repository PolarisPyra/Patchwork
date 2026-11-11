//! OpenCode 2.x: a turn runs through Patchwork's bridge to a private `opencode serve`, and its events, permissions and answers.

// Rust guideline compliant 2026-10-09

mod request;
mod parse;
#[cfg(test)]
mod tests;

pub use request::{uses_serve, bridge_request, mode_asks, steer_line, approval_reply};
pub(in crate::provider::opencode) use request::{action};
pub(in crate::provider::opencode) use parse::{parse_serve};
