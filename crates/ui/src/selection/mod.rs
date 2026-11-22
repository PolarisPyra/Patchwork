//! Selecting text that is only read.
//!
//! A drag over the chat's text marks it, across paragraphs, headings, table cells and code, and
//! Ctrl+C copies what is marked. A double click marks a word, a triple click the whole paragraph,
//! and a right click opens a menu to copy it.
//!
//! Only one selection is kept at a time across the chat, the text fields and the terminal:
//! [`claim`] makes one the latest, and the others drop theirs.

// Rust guideline compliant 2026-10-09

mod element;
mod owner;
mod state;
mod text;

#[doc(inline)]
pub use element::{Selectable, selectable};
#[doc(inline)]
pub use owner::{Owner, claim, from_primary, has_marks, owner, to_primary};
#[doc(inline)]
pub use state::{
    ClearSelection, CopySelection, Kind, Mark, SelectAllText, begin_frame, bindings, clear, copy,
    margin, menu, overlay, select_all, text_id,
};
#[doc(inline)]
pub use text::word_at;
