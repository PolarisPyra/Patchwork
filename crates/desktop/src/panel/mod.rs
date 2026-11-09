//! The panel under the chat: the integrated terminal, the Output view
//! with this launch's logs, and with computer use on, the Screen view. It stays alive while
//! closed, so its shells keep running and it keeps its tab.

// Rust guideline compliant 2026-10-09

mod cells;
mod header;
mod input;
mod menus;
mod output;
mod render;
mod screen;
mod state;
mod terminal;

pub use state::{Panel, PanelEvent, PanelTab};
