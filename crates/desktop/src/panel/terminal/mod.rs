//! The integrated terminal's shells. Each is a shell the server
//! runs in a pseudo-terminal; this window keeps its screen in a `vt100` parser fed by the shell's
//! frames, and draws it as rows of styled text. Keys go to the shell as the bytes a terminal
//! sends, except the app's own panel, search and settings keys.

// Rust guideline compliant 2026-10-09

mod interact;
mod shell;
mod view;

pub(super) use shell::{Metrics, Shell};
