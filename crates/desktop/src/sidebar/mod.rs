//! The sidebar: the search button, its views (tasks, changes and archived), and the usage button,
//! with the edge that resizes it.

// Rust guideline compliant 2026-10-09

mod archived_list;
pub mod projects;
mod sections;
pub mod source_control;
pub mod tasks;
pub mod usage;

use sections::{empty_row, twistie};
pub use sections::render;
pub use tasks::chat_markdown;
