//! Where the window is: the page that fills it, the sidebar's view, and the places back and
//! forward return to.
//!
//! `history.rs` keeps back and forward and the place saved for the next launch; `open.rs` opens a
//! chat, an agent's page or the Quilts page in this window.

// Rust guideline compliant 2026-10-09

mod history;
mod open;

pub use patchwork_desktop_sidebar::View;

/// Somewhere the back and forward buttons return to.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Place {
    Orchestrator,
    /// A chat, by its id.
    Chat(String),
    /// A chat opened from an orchestrator card: read-only, with the sidebar closed.
    Peek(String),
    /// An agent's page, by the agent's id.
    Agent(String),
    /// The Quilts page, with the quilt it had open, by its id, or its gallery.
    Quilts(Option<String>),
}

/// What fills the window beside the activity bar.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// The sidebar, the open chat, and the panels around it.
    Chats,
    Orchestrator,
    /// What agents made to look at, drawn live.
    Quilts,
}
