//! Scheduled tasks: prompts that start on a schedule, with next-run and missed-run handling.
//!
//! Scheduled tasks: a prompt that starts a task on a schedule, the way Codex and Claude desktop
//! run theirs. The schedule is one of a fixed few shapes, so its next run can be worked out from
//! the local clock without a cron parser.
//!
//! When a schedule next runs, and what to do about a run that was missed.

// Rust guideline compliant 2026-10-09

mod types;
mod run;
mod times;
#[cfg(test)]
mod tests;

pub use types::{Task, Run, RunOutcome, Schedule, Weekday};
pub use times::{next_after, next_run_at, describe, skipped_detail, CATCH_UP_WITHIN_MS, Action, action};
#[cfg(test)]
pub(crate) use times::prepare;
