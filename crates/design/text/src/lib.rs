//! Patchwork's text: Markdown, syntax highlighting, selectable text, and how times, sizes and
//! paths are written.

// Rust guideline compliant 2026-11-27

pub mod format;
pub mod highlight;
pub mod markdown;
pub mod selection;

pub use format::{
    base_name, compact_elapsed, dir_name, display_title, effort_label, format_clock, format_date,
    format_relative, format_tokens, full_elapsed, resets_in, slug_file_name,
};
