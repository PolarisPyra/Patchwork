//! Markdown as the chat draws it: paragraphs, headings,
//! lists, quotes, rules, tables and fenced code, with bold, italic, struck, linked and code spans.

// Rust guideline compliant 2026-10-09

mod inline;
mod render;
mod events;
mod code;

pub use inline::{FindIn, find_in};
pub use render::{markdown, markdown_found, markdown_part, parts_of};
