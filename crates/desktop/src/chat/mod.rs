//! The open chat: its transcript, the usage line and the composer, with
//! search and "Open in editor" floating over the transcript's top right.

// Rust guideline compliant 2026-10-09

pub mod approvals;
pub mod background;
pub mod checks;
pub mod composer;
pub mod files;
pub mod find;
pub mod folder;
pub mod goal;
pub mod lineage;
pub mod plan;
pub mod prompts;
pub mod proposed;
pub mod queue;
pub mod rail;
mod reply;
pub mod resume;
pub mod secret;
pub mod subagents;
pub mod timeline;
mod transcript;
pub mod user;
mod view;

pub use transcript::Row;
pub use view::{OPEN_HOLD, render};
pub(crate) use view::usage_line;
