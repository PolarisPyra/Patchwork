//! An agent's home: a conversation started with it, a starting point while it has no
//! instructions, and its details beside them.

// Rust guideline compliant 2026-10-09

mod details;
mod layout;
mod sections;
mod home;
mod lifecycle;

pub use home::{AgentPage, Layout};
