//! Computer use: a screenshot of the screen, and the pointer and keyboard underneath it.
//!
//! This is what an agent needs to drive another application, the way Claude's and Codex's computer
//! use does, and it is off unless Settings → Permissions turns it on.
//!
//! A screenshot comes back as PNG at the screen's own size, never scaled. Input actions are the
//! small set a model can ask for: move, click, type, press a key, scroll.

// Rust guideline compliant 2026-10-09

mod actions;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(test)]
mod tests;

pub use actions::{set_enabled, enabled, Screenshot, Action, screenshot, describe};
pub(crate) use actions::{screenshot_to_file, perform};
