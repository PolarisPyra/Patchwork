//! Granular settings, in a card over the window: the model and effort of a chat's agent, of the
//! fallback that takes over when that agent is out of usage, and of the sub agents each starts.
//! A chat's are its own; a project's go to every chat in it and to each new one. Escape or a
//! press outside closes the card without saving.

// Rust guideline compliant 2026-10-09

mod menus;
mod render;
mod state;

pub use state::{GranularDialog, GranularScope};
