//! The dock under the chat: its tabs for the terminal, the logs and, with computer use on, the
//! screen.
//!
//! It stays alive while closed, so its shells keep running and it keeps its tab. The window
//! decides where it sits and when it opens; this crate draws it and runs what is in it.
//!
//! | File | Holds |
//! |---|---|
//! | `open_tabs.rs` | The dock's state, its tabs, and showing or hiding it |
//! | `tabs.rs` | The tab strip and each tab's buttons |
//! | `layout.rs` | The dock drawn |
//! | `menus.rs` | Opening and closing the dock's menus |
//! | `height.rs` | Its height, kept inside the window and changed by its handle |
//! | `terminal/` | The terminal tab: shells, their screens, keys, selection and find |
//! | `logs.rs` | The Output tab |
//! | `screen.rs` | The Screen tab |

// Rust guideline compliant 2026-10-09

#![expect(
    clippy::unused_self,
    reason = "a view acts through its `Context<Self>`, so some of its methods never read the view itself"
)]

mod height;
mod layout;
mod logs;
mod menus;
mod open_tabs;
mod screen;
mod tabs;
mod terminal;

pub use height::{PANEL_KEY_STEP, PANEL_MIN, SASH_HIT, kept_height};
pub use open_tabs::{Dock, DockEvent, DockTab};
