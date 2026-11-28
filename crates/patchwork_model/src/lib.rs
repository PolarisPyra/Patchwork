//! Patchwork's plain domain types: chats and messages, agents and providers, the chat apps and
//! import sources, and settings values.
//!
//! Everything here is data and pure functions over it, with no files, processes or windows, so
//! the desktop, the phone and the view crates share one set of types.

// Rust guideline compliant 2026-11-27

pub mod backgrounds;
pub mod chats;
pub mod goal;
pub mod import;
pub mod logging;
pub mod permissions;
pub mod platform;
pub mod provider;
pub mod quilts;
pub mod secrets;
pub mod terminal;
