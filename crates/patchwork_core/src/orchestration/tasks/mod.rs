//! Work an agent starts beside its reply: background tasks, subagents, and the commands
//! Patchwork's own agent runs, which can move to the background as jobs.

// Rust guideline compliant 2026-11-23

pub(in crate::orchestration) mod background;
pub(in crate::orchestration) mod chats;
pub(in crate::orchestration) mod delegate;
pub(in crate::orchestration) mod dispatch;
pub(in crate::orchestration) mod jobs;
pub(in crate::orchestration) mod subagents;
pub(in crate::orchestration) mod wait;
