//! The task tree's right-click menu, the name field that renames a row in place, and the project
//! buttons that make folders and sort tasks into them by provider.

// Rust guideline compliant 2026-10-09

mod types;
mod actions;
mod rename;
mod folders;
mod view;

pub use types::{Target, ContextMenu, Renaming, SortUndo};
pub use folders::{is_agent_folder};
