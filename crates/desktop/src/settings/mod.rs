//! Settings: the dialog around the panes, its list of sections in
//! groups, its title, and which pane is open. Each pane draws itself in its own file, and the
//! controls they share are in [`controls`]. A change is saved at once and handed to the workspace
//! with [`SettingsEvent::Changed`].

// Rust guideline compliant 2026-10-09

pub(crate) mod controls;
mod chat;
mod dialog;
mod extensions;
mod general;
mod keys;
mod mcp;
pub(crate) mod permissions;
pub(crate) mod projects;
mod providers;
mod render;
mod scheduled;
pub mod shortcuts;
mod sigils;
mod ssh;
mod terminal;
mod tokens;
mod updates;

pub(crate) use general::LOG_LEVELS;
pub use dialog::{SettingsDialog, SettingsEvent};
