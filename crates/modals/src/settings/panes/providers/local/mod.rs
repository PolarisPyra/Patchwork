//! The Local providers section: a tile for each model server on
//! this computer, with a dot for whether it is answering. A saved server's tile opens its row; a
//! server running here that is not added yet adds it.

// Rust guideline compliant 2026-10-09

mod servers;
mod rows;
mod layout;

pub use servers::{LocalState};
pub use layout::{render};
