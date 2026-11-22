//! The file panel right of the chat: a file's code, its diff, or a reply's plan, with the
//! file's name, its actions, and its staged state.

// Rust guideline compliant 2026-10-09

mod file_actions;
mod area;
mod body;
mod conflicts;
mod diff;
mod header;
mod history;
mod layout;
mod open_file;

pub use open_file::{CANVAS_MAX_FRACTION, CANVAS_MIN, Canvas, CanvasTab};
