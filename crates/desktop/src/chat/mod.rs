//! The open chat: its transcript, the usage line and the composer, with search and "Open in
//! editor" in the row above the transcript.

// Rust guideline compliant 2026-10-09

pub mod approvals;
pub mod backdrop;
pub mod background;
pub mod checks;
pub mod composer;
pub mod files;
pub mod find;
pub mod folder;
pub mod goal;
mod header;
mod interrupted;
pub mod lineage;
pub mod plan;
pub mod prompts;
pub mod proposed;
pub mod queue;
pub mod rail;
mod reply;
pub mod resume;
pub mod review;
mod scheme;
pub mod secret;
pub mod subagents;
pub mod timeline;
mod transcript;
pub mod user;
mod view;

pub use transcript::{Pieces, Row};
pub use view::{OPEN_HOLD, render};
pub(crate) use view::usage_line;
