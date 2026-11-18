//! A bot's home: a conversation started with it, a starting point while it has no
//! instructions, and its details beside them.

// Rust guideline compliant 2026-10-09

mod details;
mod render;
mod sections;
mod state;

pub use state::{BotPage, Layout};
