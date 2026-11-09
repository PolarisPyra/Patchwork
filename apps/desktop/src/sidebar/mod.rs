//! The sidebar (`features/sidebar`): the search button, the task tree, and the usage button, with
//! the edge that resizes it.

mod archived;
pub mod changes;
mod export;
mod graph;
pub mod menu;
pub mod pick;
mod remove;
mod render;
mod tasks;
mod tree;
pub mod usage;

pub use render::*;
