//! The browser, which draws web pages and quilts.

// Rust guideline compliant 2026-10-09

#![expect(
    clippy::unused_self,
    reason = "a view acts through its `Context<Self>`, so some of its methods never read the view itself"
)]

pub mod browser;
