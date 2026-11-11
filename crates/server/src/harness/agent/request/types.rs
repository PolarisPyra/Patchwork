//! How a request to the provider can fail and what one brings back, with the limits on retrying it.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::time::Duration;

/// Attempts at a request that failed for a reason worth retrying. A busy endpoint, as DeepSeek's
/// is at peak hours, can take most of a minute to answer again.
pub(super) const MAX_ATTEMPTS: u32 = 6;

/// The longest wait a provider's `Retry-After` gets before a retry.
pub(super) const MAX_RETRY_AFTER: u64 = 60;

/// The longest wait between attempts when the provider does not say how long.
pub(super) const MAX_BACKOFF: u64 = 30;

/// Why a streamed reply failed, and whether any of it was shown, which rules out sending the
/// request again.
pub(in crate::harness::agent) struct StreamError {
    pub(super) message: String,
    pub(super) shown: bool,
    /// The watchdog stopped a reply stuck in a loop, which sending again would repeat.
    pub(super) stopped: bool,
}

/// The longest gap between two lines of a streamed reply before it counts as stalled. Endpoints
/// that queue or think send keep-alive comments, which count as lines.
pub(super) const STREAM_IDLE: Duration = Duration::from_secs(180);

/// How much a local model may reason without answering or calling a tool before its reply is
/// stopped: a reasoning loop holds the server until something stops it.
pub(super) const MAX_REASONING: usize = 128 * 1024;

/// A reply is checked for a loop each time it grows by this much.
pub(super) const LOOP_STEP: usize = 1024;
/// The shortest and longest stretch of text a loop repeats.
pub(super) const LOOP_UNIT: std::ops::RangeInclusive<usize> = 64..=1024;
/// Times in a row a stretch has to repeat at the end of the reply.
pub(super) const LOOP_TIMES: usize = 8;

/// Why a request failed: some failures are fixed by making the conversation smaller.
pub(in crate::harness::agent) enum Failure {
    /// The conversation is longer than the model's context window.
    Context(String),
    /// The model or endpoint does not accept images.
    Images(String),
    Other(String),
}

impl From<Failure> for Error {
    fn from(failure: Failure) -> Error {
        match failure {
            Failure::Context(m) | Failure::Images(m) | Failure::Other(m) => Error::Message(m),
        }
    }
}

impl From<String> for Failure {
    fn from(message: String) -> Failure {
        Failure::Other(message)
    }
}

/// One reply of the model.
#[derive(Default)]
pub(in crate::harness::agent) struct Reply {
    pub(in crate::harness::agent) content: String,
    pub(in crate::harness::agent) reasoning: String,
    /// The endpoint sent its reasoning as `reasoning_content`, which it wants back.
    pub(in crate::harness::agent) reasoning_content: bool,
    pub(in crate::harness::agent) calls: Vec<Call>,
    /// Tokens of the request and the reply, which the next request starts from; 0 when the
    /// endpoint reported none.
    pub(in crate::harness::agent) context: u64,
    /// The reply stopped at the model's output limit, so its text or its last call is cut off.
    pub(in crate::harness::agent) cut_off: bool,
    /// The watchdog stopped the reply, which was stuck in a loop.
    pub(super) runaway: bool,
}

impl Reply {
    /// Nothing came: no text, reasoning, or calls, as an endpoint under load sometimes answers.
    pub(super) fn is_empty(&self) -> bool {
        self.content.trim().is_empty() && self.reasoning.is_empty() && self.calls.is_empty()
    }
}

#[derive(Default, Clone)]
pub(in crate::harness::agent) struct Call {
    pub(in crate::harness::agent) id: String,
    pub(in crate::harness::agent) name: String,
    pub(in crate::harness::agent) arguments: String,
}
