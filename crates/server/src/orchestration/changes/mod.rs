//! The files a reply changed: which changes were its own, and the snapshots of the project taken
//! around each turn.

// Rust guideline compliant 2026-10-09

pub(in crate::orchestration) mod attribution;
pub mod checkpoint;
