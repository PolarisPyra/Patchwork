//! The file panel right of the chat: a file's code, its diff, or a reply's plan, with the
//! file's name, its actions, and its staged state.
//!
//! The canvas reads the open chat and the project's changes through [`CanvasHost`], and asks
//! the window to close it, reread Git or send a review with [`CanvasEvent`].

#![expect(
    clippy::unused_self,
    reason = "a view acts through its `Context<Self>`, so some of its methods never read the view itself"
)]

// Rust guideline compliant 2026-10-09

mod body;
mod conflicts;
mod diff;
mod file_actions;
mod header;
mod history;
mod host;
mod layout;
mod open_file;

pub use host::{CanvasEvent, CanvasHost};
pub use open_file::{CANVAS_MAX_FRACTION, CANVAS_MIN, Canvas, CanvasTab};
