//! Back and forward through the places the window showed, and the place saved for the next
//! launch.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::prelude::*;
use patchwork_core::chats::LastPlace;
use patchwork_client::AppState;

use crate::nav::{Page, Place};
use crate::window::Workspace;

impl Workspace {
    /// What the window shows now, as a place to come back to.
    pub(crate) fn place(&self) -> Option<Place> {
        match self.page {
            Page::Orchestrator => Some(Place::Orchestrator),
            Page::Quilts => Some(Place::Quilts(self.quilt_open.clone())),
            Page::Chats if self.agent_shown.is_some() => self.agent_shown.clone().map(Place::Agent),
            Page::Chats => self.chat.as_ref().map(|t| {
                if self.peeking {
                    Place::Peek(t.id.clone())
                } else {
                    Place::Chat(t.id.clone())
                }
            }),
        }
    }

    /// What fills the window as the next launch opens it: a chat looked in on opens as a chat.
    fn last_place(&self) -> Option<LastPlace> {
        Some(match self.place()? {
            Place::Orchestrator => LastPlace::Orchestrator,
            Place::Chat(id) | Place::Peek(id) => LastPlace::Chat(id),
            Place::Agent(id) => LastPlace::Agent(id),
            Place::Quilts(_) => return None,
        })
    }

    /// Saves what fills the window, off the UI thread, when it changed since it was last saved.
    pub(crate) fn remember_place(&mut self, cx: &mut Context<Self>) {
        let Some(place) = self.last_place() else {
            return;
        };
        if self.saved_place.as_ref() == Some(&place) {
            return;
        }
        self.saved_place = Some(place.clone());
        let core = Arc::clone(&self.core);
        cx.background_spawn(async move {
            if let Err(e) = core.set_last_place(&place) {
                log::warn!(error:% = e; "could not record what the window shows: {{error}}");
            }
        })
        .detach();
    }

    /// Opens what filled the window when the app last closed, else the orchestrator page, with
    /// the last chat ready behind it.
    pub(crate) fn restore_place(&mut self, cx: &mut Context<Self>) {
        let shown = match self.core.last_place() {
            Some(LastPlace::Chat(id)) if AppState::get(cx).chats.iter().any(|t| t.id == id) => {
                self.show(&Place::Chat(id), cx)
            }
            Some(LastPlace::Agent(id)) => self.show(&Place::Agent(id), cx),
            Some(LastPlace::Chat(_) | LastPlace::Orchestrator) | None => false,
        };
        if !shown {
            self.page = Page::Orchestrator;
        }
        self.saved_place = self.last_place();
    }
    /// Records `from` for the back button, unless the window still shows it.
    pub(crate) fn went_from(&mut self, from: Option<Place>) {
        if let Some(from) = from.filter(|from| Some(from) != self.place().as_ref()) {
            self.history_back.push(from);
            self.history_forward.clear();
        }
    }

    /// Whether there is a place to go back to.
    pub fn can_go_back(&self) -> bool {
        !self.history_back.is_empty()
    }

    /// Shows what was shown before this, as a browser's back button does.
    pub fn go_back(&mut self, cx: &mut Context<Self>) {
        self.step_history(true, cx);
    }

    /// Goes back, or to the orchestrator page when there is nothing to go back to.
    pub fn back_to_orchestrator(&mut self, cx: &mut Context<Self>) {
        if self.can_go_back() {
            self.go_back(cx);
        } else {
            self.go_to(&Place::Orchestrator, cx);
        }
    }

    /// Shows what was gone back from.
    pub fn go_forward(&mut self, cx: &mut Context<Self>) {
        self.step_history(false, cx);
    }

    /// Steps one place back or forward, skipping any chat deleted since.
    fn step_history(&mut self, back: bool, cx: &mut Context<Self>) {
        let from = self.place();
        loop {
            let next = if back {
                self.history_back.pop()
            } else {
                self.history_forward.pop()
            };
            let Some(next) = next else {
                return;
            };
            if self.show(&next, cx) {
                if let Some(from) = from {
                    if back {
                        self.history_forward.push(from);
                    } else {
                        self.history_back.push(from);
                    }
                }
                return;
            }
        }
    }
}
