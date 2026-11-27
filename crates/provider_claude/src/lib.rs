//! Claude Code CLI discovery, settings, auth probe, feature gates, and model catalog.

// Rust guideline compliant 2026-10-09

pub mod cli;
mod config;

#[doc(inline)]
pub use config::{
    auth, configured_model, credentials, entrypoint, home, mcp_signed_in, models, settings_file,
};
