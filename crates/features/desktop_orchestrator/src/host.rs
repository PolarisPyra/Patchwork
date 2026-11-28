//! What the orchestrator asks of the window that holds it, and the page's view for the chat box
//! it writes requests in.

// Rust guideline compliant 2026-10-09

use gpui::{App, EventEmitter, FocusHandle, WeakEntity, Window, div, prelude::*};
use patchwork_client::AppState;
use patchwork_client::local::core::chats::{Attachment, Chat, NewChat};
use patchwork_client::local::core::projects::Project;
use patchwork_design_theme::Theme;
use patchwork_desktop_chat::{ChatHost, Mention, Outgoing, WorktreeParts};

use crate::Orchestrator;

/// What the orchestrator asks of the window that holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrchestratorEvent {
    /// Open chat `0` as usual.
    OpenChat(String),
    /// Open chat `0` read-only, as a subagent shows.
    PeekChat(String),
    /// Open Settings at a page.
    OpenSettings(&'static str),
}

impl EventEmitter<OrchestratorEvent> for Orchestrator {}

/// The page as the chat box sees it: no chat is open, the project is the one the page picked, a
/// message is a request, and `@` names the chat or project it goes to.
pub(super) struct PageHost(pub(super) WeakEntity<Orchestrator>);

impl ChatHost for PageHost {
    fn chat<'a>(&self, _: &'a App) -> Option<&'a Chat> {
        None
    }

    fn project<'a>(&self, cx: &'a App) -> Option<&'a Project> {
        let id = self.0.upgrade()?.read(cx).project_id(cx)?;
        AppState::get(cx)
            .settings
            .projects
            .iter()
            .find(|p| p.id == id)
    }

    fn machine(&self, _: &App) -> Option<String> {
        None
    }

    fn folder_missing(&self, _: &App) -> bool {
        false
    }

    fn agent_shown(&self, _: &App) -> bool {
        false
    }

    fn has_queued(&self, _: &App) -> bool {
        false
    }

    fn editing_queued(&self, _: &App) -> bool {
        false
    }

    fn answering(&self, cx: &App) -> bool {
        self.0
            .upgrade()
            .is_some_and(|page| page.read(cx).answering_question())
    }

    fn focus(&self, _: &App) -> Option<FocusHandle> {
        None
    }

    fn worktree(&self, t: &Theme, cx: &mut App) -> WorktreeParts {
        let pinned = self
            .0
            .update(cx, |page, cx| page.pinned_chip(t, cx))
            .ok()
            .flatten();
        WorktreeParts {
            strip: None,
            files: None,
            pick: pinned.unwrap_or_else(|| div().into_any_element()),
            told: None,
        }
    }

    fn answer(&self, answer: String, cx: &mut App) {
        let _ = self
            .0
            .update(cx, |page, cx| page.respond(true, Some(answer), cx));
    }

    fn send_to_agent(&self, _: String, _: Vec<Attachment>, _: NewChat, _: &mut App) -> bool {
        false
    }

    fn send(&self, message: Outgoing, _: &mut Window, cx: &mut App) {
        let _ = self.0.update(cx, |page, cx| page.submit(message, cx));
    }

    fn picks_project(&self, _: &App) -> bool {
        false
    }

    fn mentions(&self, cx: &App) -> Option<Vec<Mention>> {
        Some(self.0.upgrade()?.read(cx).mentions(cx))
    }

    fn mention(&self, id: &str, cx: &mut App) {
        let _ = self.0.update(cx, |page, cx| page.pin(id, cx));
    }
}
