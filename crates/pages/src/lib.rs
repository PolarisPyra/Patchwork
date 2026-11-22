//! The pages the window shows in place of a chat: Quilts, Android and first-run setup.
//! Each is its own entity, and tells the window what to do through its events.

// Rust guideline compliant 2026-10-09

#![expect(
    clippy::unused_self,
    reason = "a view acts through its `Context<Self>`, so some of its methods never read the view itself"
)]

pub mod android;
pub mod onboarding;
pub mod quilts;
