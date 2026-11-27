//! Agents: a named agent with standing instructions, files it knows, and the projects it may work
//! in. Each run of an agent is a chat of its own that names the agent.

// Rust guideline compliant 2026-10-09

mod knowledge;
mod manage;
mod run;
mod types;

pub(crate) use knowledge::agents_folder;

#[doc(inline)]
pub use types::{
    BLOCKED_REASON, BlockedTools, Agent, AgentRun, AgentUsage, KnowledgeFile, STARTING_POINTS,
    StartingPoint,
};
