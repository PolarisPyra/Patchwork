//! Runs agent turns by spawning the agent CLIs and translating their JSONL output into
//! `AgentEvent`s. Each turn is one child process; its stdin stays open while the turn runs so
//! approvals can be answered. `Runner` is the entry point.
//!
//! The code is grouped by feature, one folder each, and this file is the index of all of them:
//! the files a feature is made of are declared in its block below.

/// Running one agent turn: the `Runner` that holds every agent process, starting a turn and the
/// instructions it launches with, its process and input, how it ends, and what a new app session
/// does with turns the last one left behind.
pub(crate) mod turn {
    pub(in crate::orchestration) mod end;
    pub(in crate::orchestration) mod instructions;
    pub(in crate::orchestration) mod process;
    pub(crate) mod recovery;
    pub(in crate::orchestration) mod runner;
    pub(in crate::orchestration) mod start;
    pub(in crate::orchestration) mod stuck;
}

/// The user's messages: sending one, and those sent while a turn runs, steered into it or queued
/// behind it.
mod messages {
    pub(in crate::orchestration) mod queue;
    pub(in crate::orchestration) mod queued;
    pub(in crate::orchestration) mod send;
}

/// What an agent reports: reading its output, the events streamed to the UI and saved on the
/// thread, and the tokens it used.
mod output {
    pub(in crate::orchestration) mod events;
    pub(in crate::orchestration) mod ingest;
    pub(in crate::orchestration) mod log;
    pub(in crate::orchestration) mod stream;
    pub(in crate::orchestration) mod usage;
}

/// Approvals a turn waits on, answered by the user or by the permission rules, including the
/// permission hooks Codex and Antigravity run before a tool call, and the agent's requests for a
/// secret.
mod approvals {
    pub(in crate::orchestration) mod answer;
    pub(in crate::orchestration) mod hooks;
    pub(in crate::orchestration) mod secrets;
}

/// The files a reply changed: which changes were its own, and the snapshots of the project taken
/// around each turn.
pub mod changes {
    pub(in crate::orchestration) mod attribution;
    pub mod checkpoint;
}

/// Work an agent starts beside its reply: background tasks, subagents, and the commands
/// Patchwork's own agent runs, which can move to the background as jobs.
mod tasks {
    pub(in crate::orchestration) mod background;
    pub(in crate::orchestration) mod jobs;
    pub(in crate::orchestration) mod subagents;
}

/// A thread's `/goal`: what the app keeps about it, how turns pursue it, and stopping it.
pub(crate) mod goal {
    mod run;
    mod state;
    mod stop;

    pub(in crate::orchestration) use run::{end_input, pursued, settle};
    pub(in crate::orchestration) use stop::clear_command;
    pub(crate) use state::{
        clear_dismissal, is_dismissed, next_goal, signature, update_is_dismissed,
        update_releases_held,
    };
    pub use stop::is_upkeep;
}

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
