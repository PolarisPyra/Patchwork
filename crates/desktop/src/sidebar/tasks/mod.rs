//! The Tasks view: the projects and their chats, the Agents section, picking and removing rows,
//! their right-click menu, and exporting a chat.

// Rust guideline compliant 2026-10-09

mod agents_section;
pub(super) mod chat_row;
mod export_chat;
pub mod multi_select;
mod remove_rows;
pub mod row_menu;
pub(super) mod task_list;

pub use export_chat::chat_markdown;
