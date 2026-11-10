//! The window: the toolbar, the activity bar, the sidebar, the chat and the status bar, over the
//! state they all read.

// Rust guideline compliant 2026-10-09

mod actions;
mod add_project;
mod background_dialog;
mod bars;
mod interrupted;
mod canvas_area;
mod onboarding;
mod palette;
mod panel_area;
mod projects;
mod render;
mod state;
mod threads;
mod updates;

pub use state::{SIDEBAR_MAX, SIDEBAR_MIN, SIDEBAR_WIDTH, View, Workspace, is_busy, is_working};
