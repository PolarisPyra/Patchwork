//! The box a message is written in: the editor, the files
//! attached to the message, and the toolbar of pickers, formatting, attach, stop and send.

// Rust guideline compliant 2026-10-09

mod chips;
mod menus;
mod model_notice;
mod model_picker;
mod recall;
mod render;
mod send;
mod sigil_menu;
mod state;

pub use state::{Composer, Picks};
