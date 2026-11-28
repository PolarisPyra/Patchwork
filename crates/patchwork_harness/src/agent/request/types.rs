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
pub(in crate::agent) struct StreamError {
    pub(super) message: String,
    pub(super) shown: bool,
    /// The user stopped the turn; the text the reply had shown by then.
    pub(super) interrupted: Option<String>,
}

/// The longest gap between two lines of a streamed reply from a remote endpoint before it counts
/// as stalled. Endpoints that queue or think send keep-alive comments, which count as lines.
/// Local endpoints are not timed out mid-stream.
pub(super) const STREAM_IDLE: Duration = Duration::from_secs(180);

/// How often a wait on the provider, for its next line or before a retry, checks whether the user
/// stopped the turn.
pub(super) const STOP_POLL: Duration = Duration::from_millis(100);

/// Why a request ends when the user stopped the turn.
pub(super) const STOPPED: &str = "The user stopped the turn";

/// Why a request failed: some failures are fixed by making the conversation smaller.
pub(in crate::agent) enum Failure {
    /// The conversation is longer than the model's context window.
    Context(String),
    /// The model or endpoint does not accept images.
    Images(String),
    Other(String),
    /// The user stopped the turn; the text the reply had shown by then.
    Interrupted(String),
}

impl From<Failure> for Error {
    fn from(failure: Failure) -> Error {
        match failure {
            Failure::Context(m) | Failure::Images(m) | Failure::Other(m) => Error::Message(m),
            Failure::Interrupted(_) => Error::Message(STOPPED.into()),
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
pub(in crate::agent) struct Reply {
    pub(in crate::agent) content: String,
    pub(in crate::agent) reasoning: String,
    /// The endpoint sent its reasoning as `reasoning_content`, which it wants back.
    pub(in crate::agent) reasoning_content: bool,
    pub(in crate::agent) calls: Vec<Call>,
    /// Tokens of the request and the reply, which the next request starts from; 0 when the
    /// endpoint reported none.
    pub(in crate::agent) context: u64,
    /// The reply stopped at the model's output limit, so its text or its last call is cut off.
    pub(in crate::agent) cut_off: bool,
}

impl Reply {
    /// Nothing came: no text, reasoning, or calls, as an endpoint under load sometimes answers.
    pub(super) fn is_empty(&self) -> bool {
        self.content.trim().is_empty() && self.reasoning.is_empty() && self.calls.is_empty()
    }
}

#[derive(Default, Clone)]
pub(in crate::agent) struct Call {
    pub(in crate::agent) id: String,
    pub(in crate::agent) name: String,
    pub(in crate::agent) arguments: String,
}
