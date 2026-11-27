//! The model picker: a row of swatches, one per agent and one for the starred models kept across
//! restarts, over a searchable list of the swatch's models, and the effort slider at its foot.

// Rust guideline compliant 2026-10-09

mod effort_meter;
mod layout;
mod nav;
mod open;
mod rows;

pub use effort_meter::effort_name;
pub use rows::{ModelPicker, ordered, read_favorites};
