//! A multi-line plain-text field: the composer's editor. It takes text from the platform's input
//! method, so typing, pasting and composing in other scripts work, and wraps lines to its width.

// Rust guideline compliant 2026-10-09

mod state;
mod keys;
mod input;
mod history;
mod markdown;
mod element;

pub use state::{
    Arrow, BackspaceEmpty, Changed, Edge, Paste, Submitted, SubmittedBack, Tabbed, TextArea,
    bindings,
};
