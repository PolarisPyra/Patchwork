//! Add a project: picks a project folder inside the app, laid out like search.
//!
//! The field holds the folder being shown: type after it to filter its folders, or type or paste another path and press
//! Enter to go there. When `~/.ssh/config` names hosts, a list of machines comes first, and a host's
//! folders are browsed over SSH.

// Rust guideline compliant 2026-10-09

mod folder_browser;
mod layout;

pub use folder_browser::{Closed, AddProject};
