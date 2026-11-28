//! The orchestrator page: a single field over an animated butterfly. A request written in it goes
//! to the chat it names, or starts a chat in the project it names, without opening either.

#![expect(
    clippy::unused_self,
    reason = "a view acts through its `Context<Self>`, so some of its methods never read the view itself"
)]

// Rust guideline compliant 2026-10-09

mod attachments;
mod cards;
mod chips;
mod host;
mod layout;
mod pill;
mod requests;

pub use host::{Launch, LaunchMenu, OrchestratorEvent, OrchestratorHost};
pub use layout::OrchestratorBackdrop;
pub use requests::Orchestrator;
