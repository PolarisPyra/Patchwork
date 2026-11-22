//! Files and folders on the user's machine.
//!
//! Paths the user or an agent typed, Patchwork's own folders, browsing a machine's folders, and
//! opening a file in the user's editor.

// Rust guideline compliant 2026-10-09

pub mod browse;
pub mod dirs;
pub(crate) mod editor;
pub mod paths;
