//! What the agent page reads from the window that holds it, and what it asks of it.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, AnyView, App, Entity, EventEmitter};
use patchwork_client::local::core::agents::Agent;
use patchwork_model::backgrounds::ChatBackground;
use patchwork_themes::Scheme;
use patchwork_design_editor::TextArea;
use patchwork_design_theme::Theme;

use crate::AgentPage;

/// What the agent page reads from the window that holds it, as it is when the page draws.
pub trait AgentHost {
    /// The window's message field, drawn under the agent's conversation.
    fn composer(&self, cx: &App) -> Option<AnyView>;

    /// The text field inside the message field.
    fn composer_editor(&self, cx: &App) -> Option<Entity<TextArea>>;

    /// The scheme `background` is drawn for, or the open chat's when `background` is None.
    fn scheme(&self, background: Option<&ChatBackground>, cx: &App) -> Option<Scheme>;

    /// `background` drawn behind the page, or the open chat's when `background` is None.
    fn backdrop(
        &self,
        background: Option<&ChatBackground>,
        t: &Theme,
        cx: &mut App,
    ) -> Option<AnyElement>;
}

/// What the agent page asks of the window that holds it.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// Open chat `id`, and its find bar when `find` is set.
    OpenChat {
        /// The chat to open.
        id: String,
        /// Whether to open the find bar over it.
        find: bool,
    },
    /// Show or hide the details beside the agent's chat.
    ToggleDetails,
    /// Open the background picker for agent `id`.
    EditBackground(String),
    /// Open the scheduled tasks in Settings.
    OpenRoutines,
    /// Start a new scheduled task for the agent.
    AddRoutine(Box<Agent>),
    /// A chat's messages were cleared, so the window reads its chats again.
    ChatCleared,
}

impl EventEmitter<AgentEvent> for AgentPage {}
