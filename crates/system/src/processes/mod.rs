//! Programs Patchwork runs and the ones already running.
//!
//! The command and environment they start with, the user's shells, the range and systemd scope a
//! command runs in, the process table, and the one Patchwork per data folder.

// Rust guideline compliant 2026-10-09

pub mod contained;
pub(crate) mod environment;
pub mod instance;
pub(crate) mod programs;
pub mod running;
pub mod scope;
pub(crate) mod shells;
