//! Patchwork's desktop workspace dialogs: Add a project, credentials, granular settings and interrupted replies.

// Rust guideline compliant 2026-10-09

#![expect(
    clippy::unused_self,
    reason = "a view acts through its `Context<Self>`, so some of its methods never read the view itself"
)]

pub mod add_project;
pub mod credential;
pub mod granular;
pub mod interrupted;
