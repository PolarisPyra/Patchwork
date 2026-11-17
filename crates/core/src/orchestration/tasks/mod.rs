//! Work an agent starts beside its reply: background tasks, subagents, the commands
//! Patchwork's own agent runs, which can move to the background as jobs, and the review of what
//! the reply changed.

// Rust guideline compliant 2026-10-09

pub(in crate::orchestration) mod background;
pub(in crate::orchestration) mod chats;
pub(in crate::orchestration) mod delegate;
pub(in crate::orchestration) mod jobs;
pub(in crate::orchestration) mod review;
pub(in crate::orchestration) mod subagents;
pub(in crate::orchestration) mod wait;
