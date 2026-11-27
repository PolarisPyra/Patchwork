//! The task tree's right-click menu, the name field that renames a row in place, and the project
//! buttons that make folders and sort tasks into them by provider.

// Rust guideline compliant 2026-10-09

mod target;
mod items;
mod rename_field;
mod provider_folders;
mod popup;

pub use target::{Target, ContextMenu, Renaming, SortUndo};
