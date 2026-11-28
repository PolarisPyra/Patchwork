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

pub use actions::{Screenshot, look, perform, screenshot, screenshot_to_file};
pub use patchwork_harness::computer::{Action, describe, enabled, seeing, set_enabled, set_seeing};

/// The screen and the pointer and keyboard, for Patchwork's own agent.
#[derive(Debug, Clone, Copy)]
pub struct Hands;

impl patchwork_harness::computer::Hands for Hands {
    fn perform(&self, action: &Action) -> Result<(), crate::Error> {
        perform(action)
    }

    fn screenshot_to_file(
        &self,
        dir: &std::path::Path,
    ) -> Result<std::path::PathBuf, crate::Error> {
        screenshot_to_file(dir)
    }
}
