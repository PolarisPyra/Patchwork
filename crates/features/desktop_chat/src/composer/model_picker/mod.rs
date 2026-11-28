//! The model menu: the starred models kept across restarts, each agent's models, and the effort,
//! in the app's context-menu look.

// Rust guideline compliant 2026-10-09

pub mod effort_meter;
pub mod layout;
pub mod rows;

pub use effort_meter::effort_name;
pub use rows::{ordered, read_favorites};
