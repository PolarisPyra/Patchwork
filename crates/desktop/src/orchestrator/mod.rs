//! The orchestrator page: a single field over an animated butterfly. A request written in it goes
//! to the chat it names, or starts a chat in the project it names, without opening either.

// Rust guideline compliant 2026-10-09

mod background;
mod butterfly;
mod cards;
mod pill;
mod render;
mod scene;
mod sky;
mod state;

pub use background::{PreviewFlight, butterfly_preview};
pub use scene::scene_preview;
pub use state::Orchestrator;
