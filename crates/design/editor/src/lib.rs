//! A multi-line plain-text field: the composer's editor. It takes text from the platform's input
//! method, so typing, pasting and composing in other scripts work, and wraps lines to its width.

// Rust guideline compliant 2026-10-09

#![expect(
    clippy::unused_self,
    reason = "a view acts through its `Context<Self>`, so some of its methods never read the view itself"
)]

mod element;
mod field;
mod history;
mod input;
mod keys;
mod markdown;
mod mouse;

pub use field::{
    Arrow, BackspaceEmpty, Changed, Edge, Paste, Submitted, SubmittedBack, Tabbed, TextArea,
    bindings,
};
