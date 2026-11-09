//! Runs agent turns by spawning the agent CLIs and translating their JSONL output into
//! `AgentEvent`s. Each turn is one child process; its stdin stays open while the turn runs so
//! approvals can be answered. `Runner` is the entry point.
//!
//! The code is grouped by feature, one folder each, and each folder's `mod.rs` declares the files
//! it is made of.

mod approvals;
pub mod changes;
pub(crate) mod goal;
mod messages;
mod output;
mod tasks;
pub(crate) mod turn;

use approvals::answer::{decide, unasked};
pub use approvals::answer::ApprovalDecision;
pub use goal::is_upkeep;
pub use messages::queue::FollowUpInput;
pub use output::events::{AgentEvent, AgentEventEnvelope};
pub(crate) use output::log::log_event;
pub(crate) use output::stream::publish;
pub(crate) use turn::instructions::{agent_instructions, system_prompt};
pub use turn::runner::{Delivery, Runner};
pub use turn::start::TurnRequest;
