//! What the canvas reads from the window that holds it, and what it asks of it.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{App, EventEmitter};
use patchwork_model::chats::Message;
use patchwork_client::local::core::source_control::FileChange;

use crate::Canvas;

/// What the canvas reads from the window that holds it, as it is when the canvas draws.
pub trait CanvasHost {
    /// The open chat's message `id`, as it is now.
    fn message(&self, id: &str, cx: &App) -> Option<Arc<Message>>;

    /// The file at `path` in the project's uncommitted changes.
    fn change(&self, path: &str, cx: &App) -> Option<FileChange>;
}

/// What the canvas asks of the window that holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanvasEvent {
    /// The user closed the canvas.
    Close,
    /// An action on the file changed what Git holds, so Source Control reads it again.
    GitChanged,
    /// The user sent their review notes for the project.
    Review {
        /// The project the notes are about.
        project: String,
        /// The notes, as one message.
        text: String,
    },
}

impl EventEmitter<CanvasEvent> for Canvas {}
