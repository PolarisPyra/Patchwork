//! The user's messages: sending one, and those sent while a turn runs, steered into it or queued
//! behind it.

// Rust guideline compliant 2026-10-09

pub(in crate::orchestration) mod queue;
pub(in crate::orchestration) mod queued;
pub(in crate::orchestration) mod send;
