//! Granular settings, in a card over the window: the agent, model and effort of a chat's
//! orchestrator, which plans and reviews, and of its executor, which its sub agents run on.
//!
//! A chat's are its own; a project's go to every chat in it and to each new one. Escape or a
//! press outside closes the card without saving.

// Rust guideline compliant 2026-11-23

mod menus;
mod layout;
mod card;

pub use card::{First, GranularDialog, GranularEvent, GranularScope};
