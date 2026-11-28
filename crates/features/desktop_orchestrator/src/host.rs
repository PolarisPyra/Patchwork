//! What the orchestrator reads from the window that holds it, and what it asks of it.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, App, ClickEvent, EventEmitter, Window};
use patchwork_client::local::core::chats::NewChat;
use patchwork_model::provider::AgentKind;

use crate::Orchestrator;

/// One of the menus under the pill that change what a request's new chat starts with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMenu {
    /// The agent and model.
    Model,
    /// The reasoning effort.
    Effort,
    /// The project.
    Project,
}

/// What a request that names no chat starts its new chat with, and which menu is open.
#[derive(Debug, Clone)]
pub struct Launch {
    /// The agent the chat starts with.
    pub agent: AgentKind,
    /// The agent's model, or None for its default.
    pub model: Option<String>,
    /// The model's name as the picker shows it.
    pub model_name: String,
    /// The effort's name, or None when the model takes no effort.
    pub effort: Option<String>,
    /// The project the chat starts in, or None for a chat in no project.
    pub project: Option<String>,
    /// The menu open under the pill, if any.
    pub open: Option<LaunchMenu>,
}

/// What the orchestrator reads from the window that holds it: the chat box's launch choices,
/// which the orchestrator draws as chips under its pill.
pub trait OrchestratorHost {
    /// What a request's new chat starts with, as the chips show it.
    fn launch(&self, cx: &App) -> Launch;

    /// `menu`, drawn under its chip while it is open.
    fn launch_menu(
        &self,
        menu: LaunchMenu,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement>;

    /// Opens `menu`, or closes it when it is open.
    fn toggle_launch_menu(
        &self,
        menu: LaunchMenu,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut App,
    );

    /// The chat a request that names none starts.
    fn new_chat(&self, cx: &App) -> NewChat;
}

/// What the orchestrator asks of the window that holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrchestratorEvent {
    /// Open chat `0` as usual.
    OpenChat(String),
    /// Open chat `0` read-only, as a subagent shows.
    PeekChat(String),
}

impl EventEmitter<OrchestratorEvent> for Orchestrator {}
