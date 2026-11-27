//! App lifecycle and host operations for Patchwork: updates, restarts, crash containment, and
//! session logs.

// Rust guideline compliant 2026-10-09

pub mod crash;
pub mod logging;
pub mod restart;
pub mod updates;
