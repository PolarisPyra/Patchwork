//! The tasks an agent started and left running: what each one is, whether it holds the turn's stdin
//! open, and which of them are listed under the message they started in.
//!
//! Claude reports each task in the stream, and the CLI reports its end by itself, so a task it
//! reports holds stdin open until it ends. Claude also sends the full set of tasks still running
//! whenever it changes, which ends a task whose own end never reached us. Codex reports a command
//! or subagent only through the call that started it. One still open when its turn completes is
//! listed, but Codex never reports it ending, so it holds nothing and the turn ends with it.

// Rust guideline compliant 2026-10-09

mod tracker;
#[cfg(test)]
mod tests;

pub(in crate::orchestration) use tracker::{Background};
