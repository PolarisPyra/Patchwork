//! The dock under the chat: its tabs for the terminal, the logs and, with computer use on, the
//! screen, and its place in the window.
//!
//! It stays alive while closed, so its shells keep running and it keeps its tab. What each tab
//! shows is its own feature: `crate::terminal`, `crate::logs` and `crate::screen`.

// Rust guideline compliant 2026-10-09

mod menus;
pub(crate) mod place;
mod layout;
mod open_tabs;
mod tabs;

pub(crate) use open_tabs::{Dock, DockEvent, DockTab, MenuFor};
