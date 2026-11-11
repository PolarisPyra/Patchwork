//! A thread's claim on starting a turn, so that two turns never start in it at once.

// Rust guideline compliant 2026-10-09

use crate::Error;

use std::sync::PoisonError;

use crate::orchestration::Runner;

/// A thread's place in `Runner::starting`, held until its turn is in `turns` or has failed to start.
pub(in crate::orchestration) struct Starting<'a> {
    runner: &'a Runner,
    thread_id: String,
}

impl<'a> Starting<'a> {
    pub(in crate::orchestration) fn claim(
        runner: &'a Runner,
        thread_id: &str,
    ) -> Result<Self, Error> {
        let mut starting = runner
            .starting
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !starting.insert(thread_id.to_string()) {
            return Err(STILL_STARTING.into());
        }
        Ok(Self {
            runner,
            thread_id: thread_id.to_string(),
        })
    }
}

impl Drop for Starting<'_> {
    fn drop(&mut self) {
        self.runner
            .starting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.thread_id);
        // A stop asked of a start that failed has nothing left to stop.
        self.runner
            .stop_starting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.thread_id);
    }
}

/// Refuses a send to a thread whose turn is still starting, rather than starting a second agent.
pub(crate) const STILL_STARTING: &str =
    "This chat is still starting its turn; send again in a moment";
