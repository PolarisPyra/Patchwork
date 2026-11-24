//! Settings: the dialog around the panes, its sections, its title, and which pane is open.
//!
//! Each pane draws itself in its own file or folder under `panes/`, and the
//! controls they share are in [`controls`]. A change is saved at once and handed to the workspace
//! with [`SettingsEvent::Changed`].

// Rust guideline compliant 2026-10-09

mod controls;
mod dialog;
mod keyboard;
mod nav;
mod panes;
mod layout;

pub use dialog::{SettingsDialog, SettingsEvent};
pub use panes::general::show_app_icon;
pub use panes::permissions::{kind_label, scope_name};
pub use panes::projects::{import_sources, source_agent, source_label};
pub use panes::shortcuts::{bind_keys, command_action, key_label, page_commands};
