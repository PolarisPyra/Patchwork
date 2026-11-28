//! The menus the word before the caret opens: project files and folders for "@", enabled skills for
//! "$", custom agents for "&" and commands for "/", listed above the composer.
//!
//! Up and Down move
//! the highlight, Tab or Enter picks it, and Escape hides the menu until the draft changes.

// Rust guideline compliant 2026-10-09

pub mod commands;
pub mod open_menu;
pub mod layout;
pub mod keys;

pub use commands::{local_command};
pub use open_menu::{SigilMenu};
