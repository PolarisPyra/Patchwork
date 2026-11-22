//! Settings: the dialog around the panes, its list of sections in
//! groups, its title, and which pane is open. Each pane draws itself in its own file, and the
//! controls they share are in [`controls`]. A change is saved at once and handed to the workspace
//! with [`SettingsEvent::Changed`].

// Rust guideline compliant 2026-10-09

#![expect(
    clippy::unused_self,
    reason = "a view acts through its `Context<Self>`, so some of its methods never read the view itself"
)]

mod controls;
mod chat;
mod costs;
mod dialog;
mod extensions;
mod gateway;
mod general;
mod keys;
mod mcp;
mod nav;
mod notifications;
mod permissions;
mod projects;
mod providers;
mod render;
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

pub use dialog::{SettingsDialog, SettingsEvent};
pub use general::show_app_icon;
pub use permissions::{kind_label, scope_name};
pub use projects::{import_sources, source_agent, source_label};
pub use shortcuts::{bind_keys, key_label};
