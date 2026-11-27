//! Scheduled tasks: prompts that start on a schedule, with next-run and missed-run handling.
//!
//! Scheduled tasks: a prompt that starts a task on a schedule, the way Codex and Claude desktop
//! run theirs. The schedule is one of a fixed few shapes, so its next run can be worked out from
//! the local clock without a cron parser.
//!
//! When a schedule next runs, and what to do about a run that was missed. A task can instead
//! start on an event: a pull request opening or a check failing in its project's repository,
//! or a webhook called on this computer.

// Rust guideline compliant 2026-10-09

mod types;
mod run;
mod times;
mod events;
mod webhook;

pub use types::{Task, Run, RunOutcome, Schedule, Trigger, Weekday};
pub use times::{
    next_after, next_run_at, next_for, describe, describe_task, skipped_detail, CATCH_UP_WITHIN_MS,
    Action, action,
};
