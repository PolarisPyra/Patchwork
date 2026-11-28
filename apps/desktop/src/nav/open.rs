//! Opening a place in the window: a chat, a chat looked in on, the orchestrator, an agent's page
//! or the Quilts page.

// Rust guideline compliant 2026-10-09

use patchwork_desktop_chat::prelude::*;
use gpui::prelude::*;
use patchwork_client::AppState;

use crate::nav::{Page, Place};
use crate::window::Workspace;

impl Workspace {
    /// Opens chat `id`, recording what the window showed for the back button.
    pub fn open_chat(&mut self, id: &str, cx: &mut Context<Self>) {
        self.go_to(&Place::Chat(id.to_string()), cx);
    }

    /// Opens chat `id` read-only with the sidebar closed, as a subagent shows, for looking in on
    /// it from the orchestrator page.
    pub fn peek_chat(&mut self, id: &str, cx: &mut Context<Self>) {
        self.go_to(&Place::Peek(id.to_string()), cx);
    }

    /// Shows `to`, and records what the window showed before for the back button.
    pub(crate) fn go_to(&mut self, to: &Place, cx: &mut Context<Self>) {
        let from = self.place();
        if self.show(to, cx) {
            self.went_from(from);
        }
    }

    /// Shows `place`, and whether it could be shown.
    pub(crate) fn show(&mut self, place: &Place, cx: &mut Context<Self>) -> bool {
        match place {
            Place::Orchestrator => {
                self.page = Page::Orchestrator;
                cx.notify();
                true
            }
            Place::Chat(id) => {
                // The open chat's tab, or else the one left to start a new chat, shows it.
                // A chat with a tab in another pane shows there, as that pane left it.
                let kept = self.enter_pane_of(id, cx);
                let left = self.left_tab.take();
                let beside = self.chat.as_ref().map(|c| c.id.clone()).or(left);
                let new = std::mem::take(&mut self.new_tab)
                    || AppState::get(cx).settings.tasks_in_new_tab;
                let shown = self.show_chat(id, cx);
                if shown {
                    self.keep_tab(beside.as_deref(), new, cx);
                    if let Some(kept) = kept {
                        self.restore_view(kept);
                    }
                }
                shown
            }
            Place::Agent(id) => self.show_agent(id, cx),
            Place::Quilts(open) => self.show_quilts(open.as_ref(), cx),
            Place::Peek(id) => {
                let shown = self.show_chat(id, cx);
                self.peeking |= shown;
                shown
            }
        }
    }
}
