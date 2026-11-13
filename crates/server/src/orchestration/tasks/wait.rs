//! Waiting on another chat's turn: a sub agent's, a reviewer's, or a chat an agent started or
//! messaged, until it ends with a reply.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::Server;
use crate::orchestration::Runner;
use crate::threads::{Message, MessageStatus, Role};

/// How often the wait looks at the turn, and at whether its caller still waits.
const POLL: Duration = Duration::from_millis(500);

/// How long a turn may take to show a reply before it counts as not started.
const START_WAIT: Duration = Duration::from_secs(60);

/// Why a wait ended without a reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::orchestration) enum Unanswered {
    /// The caller stopped waiting.
    Stopped,
    /// The turn ran past the wait's limit.
    TimedOut,
    /// No reply showed within [`START_WAIT`].
    NotStarted,
}

impl Runner {
    /// Waits until the turn in `thread_id` ends, and returns the reply it ended with: the last
    /// one after the message `after`, or in the whole chat without it. Gives up when `gone` says
    /// the caller stopped waiting, or after `limit`. The turn itself is left running either way.
    pub(in crate::orchestration) fn wait_for_reply(
        &self,
        server: &Arc<Server>,
        thread_id: &str,
        after: Option<&str>,
        gone: &dyn Fn() -> bool,
        limit: Duration,
    ) -> Result<Message, Unanswered> {
        let started = Instant::now();
        // A turn that is rerun, as in a new session, is for a moment neither running nor
        // starting, so its end is only taken once it is seen twice.
        let mut ended = false;
        loop {
            std::thread::sleep(POLL);
            if gone() {
                return Err(Unanswered::Stopped);
            }
            if started.elapsed() >= limit {
                return Err(Unanswered::TimedOut);
            }
            if self.is_starting(thread_id) || self.is_running(thread_id) {
                ended = false;
                continue;
            }
            let reply = server.store.thread(thread_id).and_then(|t| {
                let from = after
                    .and_then(|id| t.messages.iter().position(|m| m.id == id))
                    .map_or(0, |at| at + 1);
                t.messages
                    .get(from..)
                    .and_then(|later| later.iter().rfind(|m| !m.queued))
                    .filter(|m| m.role == Role::Assistant && m.status != MessageStatus::Streaming)
                    .cloned()
            });
            match reply {
                Some(reply) if ended => return Ok(Arc::unwrap_or_clone(reply)),
                Some(_) => ended = true,
                None if started.elapsed() >= START_WAIT => return Err(Unanswered::NotStarted),
                None => {}
            }
        }
    }
}
