//! The terminal tab under the chat. Each is a shell the core
//! runs in a pseudo-terminal; this window keeps its screen in a `vt100` parser fed by the shell's
//! frames, and draws it as rows of styled text. Keys go to the shell as the bytes a terminal
//! sends, except the app's own dock, search and settings keys.

// Rust guideline compliant 2026-10-09

pub(crate) mod cells;
pub(crate) mod input;
mod interact;
mod select;
mod shell;
mod active_shell;

pub(crate) use select::Drag;
pub(crate) use shell::{Metrics, Shell, same_folder};
