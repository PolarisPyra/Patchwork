//! The windows beside the chat: the panel under it, with the terminal, the Output view and the
//! Screen view, and the browser that draws web pages and quilts.

// Rust guideline compliant 2026-10-09

#![expect(
    clippy::unused_self,
    reason = "a view acts through its `Context<Self>`, so some of its methods never read the view itself"
)]

pub mod browser;
pub mod panel;
