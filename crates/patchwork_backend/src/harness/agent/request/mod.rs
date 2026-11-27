//! Talking to the endpoint: the request body, retries, and reading the streamed reply.

// Rust guideline compliant 2026-10-09

mod types;
mod body;
mod watch;
pub(in crate::harness) mod send;

pub(in crate::harness::agent) use types::{Failure, Reply, Call};
