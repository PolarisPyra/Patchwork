//! The background picker, shared by Settings → Chat theme and a chat's own background dialog.
//!
//! The default, the butterfly, the animated backgrounds, the presets, and the pictures picked from
//! disk, each removable with a short while to take it back.

// Rust guideline compliant 2026-10-09

mod layout;
mod choices;

pub use self::choices::{BackgroundPicker, PickerEvent};
