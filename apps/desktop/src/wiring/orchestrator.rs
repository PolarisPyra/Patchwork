//! The orchestrator page in the window, and what it asks of the workspace.

// Rust guideline compliant 2026-10-09

use gpui::{Entity, Window, prelude::*};
use patchwork_desktop_orchestrator::{Orchestrator, OrchestratorEvent};

use crate::window::Workspace;

impl Workspace {
    /// Makes the orchestrator page, and handles what it asks of the window.
    pub(crate) fn new_orchestrator(
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Orchestrator> {
        let orchestrator = cx.new(|cx| Orchestrator::new(window, cx));
        cx.subscribe_in(
            &orchestrator,
            window,
            |this, _, event: &OrchestratorEvent, window, cx| match event {
                OrchestratorEvent::OpenChat(id) => this.open_chat(id, cx),
                OrchestratorEvent::PeekChat(id) => this.peek_chat(id, cx),
                OrchestratorEvent::OpenSettings(page) => {
                    this.open_settings(Some(page), window, cx);
                }
            },
        )
        .detach();
        orchestrator
    }
}
