//! The open chat: its transcript, the usage line and the composer, with search and "Open in
//! editor" in the row above the transcript.
//!
//! `transcript/` is the message list and each reply, `top_right/` what floats over the
//! transcript's top right, `above_composer/` the strips and cards between the transcript and the
//! composer, `composer/` the box a message is written in, `tabs.rs` the strip of chats open in
//! tabs over it all, and `panes/` the chat area split into panes, each with tabs of its own.

// Rust guideline compliant 2026-10-09

pub mod above_composer;
pub mod backdrop;
pub mod composer;
mod layout;
pub(crate) mod panes;
mod session;
mod tabs;
pub(crate) mod text_colors;
pub mod top_right;
pub mod transcript;
mod view;

pub use layout::OPEN_HOLD;
pub(crate) use tabs::TabGhost;
pub(crate) use layout::usage_line;
pub use transcript::message_list::{Pieces, Row};
pub use view::ChatView;
