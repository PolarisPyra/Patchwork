//! The model picker: the providers down the left, a search field
//! over the list of models, and the Legacy models panel beside it. Starred models are kept across
//! restarts.

// Rust guideline compliant 2026-10-09

mod rows;
mod view;
mod open;

pub use rows::{ModelPicker, ordered};
