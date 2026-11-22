//! A provider's editor: its name, address, key, models and headers, with Save, Cancel and Remove.
//! It takes the pane while it is open.

// Rust guideline compliant 2026-10-09

mod state;
mod editors;
mod render;

pub use render::render;
pub use state::{Form, PRESETS, blank, describe};
