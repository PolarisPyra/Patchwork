//! OpenCode CLI discovery, version/database detection, diagnostics, and 2.x server bridge.

// Rust guideline compliant 2026-10-09

pub mod bridge;
pub mod cli;
mod config;
mod server;

#[doc(inline)]
pub use config::{
    SHARED_DATABASE, V1_DATABASE, auth, data_dir, major_version, models, needs_own_database,
    opencode_bin,
};
