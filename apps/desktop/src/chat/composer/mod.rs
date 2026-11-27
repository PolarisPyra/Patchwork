//! The box a message is written in, as the web redesign draws it: the chat's project and machine
//! on the outer frame, then a card with the files attached, the editor, and a footer of the "+"
//! menu, the model picker with its effort, the context ring, access, stop and send.

// Rust guideline compliant 2026-10-09

mod access_picker;
mod agent_picker;
mod chips;
mod footer;
mod launch;
mod menus;
mod model_notice;
mod model_picker;
mod recall;
mod layout;
mod send;
mod sigil_menu;
mod draft;
mod workspace_row;
pub mod worktree_switch;

pub use draft::{Composer, Picks};
