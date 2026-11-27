//! Codex CLI discovery, settings, auth probe, feature flags, and model catalog.

// Rust guideline compliant 2026-10-09

pub mod cli;
mod config;

#[doc(inline)]
pub use config::{auth, config_file, configured_model, home, models, rollout_files};
