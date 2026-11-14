//! The background picker, shared by Settings → Backgrounds and a chat's own background dialog:
//! the default, the butterfly, the animated backgrounds, the presets, and the pictures picked
//! from disk, each removable with a short while to take it back.

// Rust guideline compliant 2026-10-09

mod render;
mod state;

pub use self::state::{BackgroundPicker, PickerEvent};
