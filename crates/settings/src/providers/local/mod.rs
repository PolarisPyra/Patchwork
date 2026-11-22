//! The Local providers section: a tile for each model server on
//! this computer, with a dot for whether it is answering. A saved server's tile opens its row; a
//! server running here that is not added yet adds it.

// Rust guideline compliant 2026-10-09

mod state;
mod rows;
mod render;

pub use state::{LocalState};
pub use render::{render};
