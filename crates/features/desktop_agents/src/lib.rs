//! An agent's home: a conversation started with it, a starting point while it has no
//! instructions, and its details beside them.

#![expect(
    clippy::unused_self,
    reason = "a view acts through its `Context<Self>`, so some of its methods never read the view itself"
)]

// Rust guideline compliant 2026-10-09

mod details;
mod home;
mod host;
mod layout;
mod sections;

pub use home::{AgentPage, Layout};
pub use host::{AgentEvent, AgentHost};
