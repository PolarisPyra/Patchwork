//! Subscription usage limits (session / weekly windows) per agent.
//!
//! Claude's numbers come from a rate-limited web endpoint, so results are
//! cached: reopening the usage popup reuses them, refreshing is throttled, and
//! a 429 backs off while the last good numbers stay visible.

mod types;

pub use types::{ProviderUsage, UsageWindow};

pub mod claude;
mod codex;
mod antigravity;

mod cache;

pub use cache::*;
