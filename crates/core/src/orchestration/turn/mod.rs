//! Running one agent turn: the `Runner` that holds every agent process, starting a turn and the
//! instructions it launches with, its process and input, how it ends, what a new app session
//! does with turns the last one left behind, and resuming the ones the app's closing cut off.

// Rust guideline compliant 2026-10-09

pub(in crate::orchestration) mod end;
pub(in crate::orchestration) mod instructions;
pub(in crate::orchestration) mod limit;
pub(in crate::orchestration) mod process;
pub(crate) mod recovery;
mod resume;
pub(in crate::orchestration) mod runner;
pub(in crate::orchestration) mod start;
pub(in crate::orchestration) mod stuck;
