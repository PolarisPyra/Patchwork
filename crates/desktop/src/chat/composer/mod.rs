//! The box a message is written in: the editor, the files
//! attached to the message, and the toolbar of pickers, formatting, attach, stop and send.

// Rust guideline compliant 2026-10-09

mod chips;
mod launch;
mod menus;
mod model_notice;
mod model_picker;
mod recall;
mod layout;
mod send;
mod sigil_menu;
mod draft;
pub mod worktree_switch;

pub use draft::{Composer, Picks};
