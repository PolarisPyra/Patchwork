//! Subscription usage limits (session / weekly windows) per agent.
//!
//! Claude's numbers come from a rate-limited web endpoint, so results are
//! cached: reopening the usage popup reuses them, refreshing is throttled, and
//! a 429 backs off while the last good numbers stay visible.

// Rust guideline compliant 2026-10-09

mod types;

#[doc(inline)]
pub use types::{ProviderUsage, UsageWindow};

pub mod claude;
mod codex;
mod antigravity;

mod cache;

use cache::{Failure, RATE_LIMIT_BACKOFF, window_label};
#[doc(inline)]
pub use cache::UsageCache;
