//! The file panel right of the chat: a file's code, its diff, or a reply's plan, with the
//! file's name, its actions, and its staged state.

// Rust guideline compliant 2026-10-09

mod actions;
mod body;
mod diff;
mod header;
mod render;
mod state;

pub use state::{CANVAS_MAX_FRACTION, CANVAS_MIN, Canvas, CanvasTab};
