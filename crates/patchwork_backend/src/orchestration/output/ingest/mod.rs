//! Reads an agent's output for one turn and hands each event to the modules that handle it.

// Rust guideline compliant 2026-10-09

mod approvals;
mod calls;
mod compaction;
mod dispatch;
mod exit;
mod idle;
mod read;
#[cfg(test)]
pub(in crate::orchestration) mod replays;
mod unprompted;

pub(in crate::orchestration) use read::ingest;
pub(in crate::orchestration) use unprompted::Wake;
