//! The orchestrator page: a single field over an animated butterfly. A request written in it goes
//! to the chat it names, or starts a chat in the project it names, without opening either.

// Rust guideline compliant 2026-10-09

mod butterfly;
mod render;
mod state;

pub use state::Orchestrator;
