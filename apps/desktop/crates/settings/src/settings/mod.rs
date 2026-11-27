//! Settings: the dialog around the panes, its sections, its title, and which pane is open.
//!
//! Each pane draws itself in its own file or folder here, and the
//! controls they share are in [`controls`]. A change is saved at once and handed to the workspace
//! with [`SettingsEvent::Changed`].

// Rust guideline compliant 2026-10-09

mod controls;
mod dialog;
mod keyboard;
mod nav;
mod pages;
mod chat;
mod costs;
mod devices;
mod extensions;
mod gateway;
mod general;
mod logs;
mod mcp;
mod notifications;
mod permissions;
mod projects;
mod providers;
mod scheduled;
mod security;
mod shortcuts;
mod sigils;
mod ssh;
mod terminal;
mod themes;
mod tokens;
mod updates;
mod worktrees;
mod layout;

pub use dialog::{SettingsDialog, SettingsEvent};
pub use general::show_app_icon;
pub use logs::LOG_LEVELS;
pub use permissions::{kind_label, scope_name};
pub use projects::source_label;
pub use shortcuts::{bind_keys, command_action, key_label, page_commands};
