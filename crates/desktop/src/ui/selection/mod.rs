//! Selecting text that is only read: a drag over the chat's text marks it, across paragraphs,
//! headings, table cells and code, and Ctrl+C copies what is marked. A double click marks a word,
//! a triple click the whole paragraph, and a right click opens a menu to copy it.

// Rust guideline compliant 2026-10-09

mod element;
mod state;
mod text;

#[doc(inline)]
pub use element::{Selectable, selectable};
#[doc(inline)]
pub use state::{CopySelection, Kind, Mark, begin_frame, bindings, copy, menu, overlay, text_id};
