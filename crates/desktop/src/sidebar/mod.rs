//! The sidebar (`features/sidebar`): the search button, the task tree, and the usage button, with
//! the edge that resizes it.

// Rust guideline compliant 2026-10-09

mod archived;
pub mod changes;
pub mod devices;
mod export;
mod graph;
pub mod menu;
pub mod pick;
mod remove;
mod render;
mod tasks;
mod tree;
pub mod usage;

use render::{FS_SM, FS_XS, empty_row, twistie};
pub use render::render;
