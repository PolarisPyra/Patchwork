//! Reads an agent's output for one turn and hands each event to the modules that handle it.

// Rust guideline compliant 2026-10-09

mod approvals;
mod idle;
mod read;
#[cfg(test)]
mod tests;

pub(in crate::orchestration) use read::ingest;
