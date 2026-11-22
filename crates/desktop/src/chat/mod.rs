//! The open chat: its transcript, the usage line and the composer, with search and "Open in
//! editor" in the row above the transcript.
//!
//! `transcript/` is the message list and each reply, `top_right/` what floats over the
//! transcript's top right, `above_composer/` the strips and cards between the transcript and the
//! composer, and `composer/` the box a message is written in.

// Rust guideline compliant 2026-10-09

pub mod above_composer;
pub mod backdrop;
pub mod composer;
mod layout;
mod session;
pub(crate) mod text_colors;
pub mod top_right;
pub mod transcript;

pub use layout::{OPEN_HOLD, render};
pub(crate) use layout::usage_line;
pub use transcript::message_list::{Pieces, Row};
