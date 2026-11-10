//! Work an agent starts beside its reply: background tasks, subagents, and the commands
//! Patchwork's own agent runs, which can move to the background as jobs.

// Rust guideline compliant 2026-10-09

pub(in crate::orchestration) mod background;
pub(in crate::orchestration) mod jobs;
pub(in crate::orchestration) mod subagents;
