//! The dialogs and cards over the window: Settings, granular settings, search, Add a project, the
//! background picker, git sign-in and the interrupted replies card. Each is its own entity, and tells the window
//! what to do through its events.

// Rust guideline compliant 2026-10-09

#![expect(
    clippy::unused_self,
    reason = "a view acts through its `Context<Self>`, so some of its methods never read the view itself"
)]

pub mod add_project;
pub mod background;
pub mod credential;
pub mod granular;
pub mod interrupted;
pub mod search;
pub mod settings;
