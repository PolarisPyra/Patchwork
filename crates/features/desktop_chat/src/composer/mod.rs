//! The box a message is written in, as the web redesign draws it.
//!
//! The chat's project and machine are on the outer frame, then a card with the files attached,
//! the editor, and a footer of the "+" menu, the model picker with its effort, the context ring,
//! access, stop and send.

// Rust guideline compliant 2026-10-09

pub mod access_picker;
pub mod agent_picker;
pub mod chips;
pub mod footer;
pub mod launch;
pub mod menus;
pub mod model_notice;
pub mod model_picker;
pub mod recall;
pub mod layout;
pub mod send;
pub mod sigil_menu;
pub mod draft;
pub mod workspace_row;

pub use draft::{Composer, Picks};
