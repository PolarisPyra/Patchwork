//! The right-click menu of the sidebar's rows, the chat tabs and the panes: what it offers, what
//! each item does, removing rows, and exporting a chat.

// Rust guideline compliant 2026-10-09

mod export_chat;
mod items;
mod popup;
mod remove_rows;
mod target;

pub use export_chat::chat_markdown;
pub use target::{ContextMenu, Target};
