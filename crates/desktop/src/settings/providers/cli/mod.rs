//! A CLI's Details: its version and path, its flags or settings, its
//! models, and its own read-only diagnostics. It takes the pane while it is open.

// Rust guideline compliant 2026-10-09

mod read;
mod details;
mod render;

pub use read::{has_details, Details};
pub use render::{render};
