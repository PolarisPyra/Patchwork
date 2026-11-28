//! The composer: made with its view of the window, and what the window does when it asks.

// Rust guideline compliant 2026-10-09

use patchwork_desktop_chat::prelude::*;
use std::rc::Rc;

use gpui::{App, Entity, FocusHandle, WeakEntity, Window, prelude::*};
use patchwork_core::chats::{Attachment, Chat, NewChat};
use patchwork_core::projects::Project;
use patchwork_desktop_chat::{ChatEvent, ChatHost, Composer, Outgoing, WorktreeParts};
use patchwork_design_theme::Theme;

use patchwork_desktop_chat::above_composer::worktree_status::worktree_strip;
use patchwork_desktop_chat::worktree_switch::{worktree_pick_chip, worktree_told_line};
use crate::window::Workspace;

/// The composer's view of the workspace: the open chat, its project, and the panels above the
/// composer.
struct Host(WeakEntity<Workspace>);

impl ChatHost for Host {
    fn chat<'a>(&self, cx: &'a App) -> Option<&'a Chat> {
        self.0.upgrade()?.read(cx).chat.as_ref()
    }

    fn project<'a>(&self, cx: &'a App) -> Option<&'a Project> {
        self.0.upgrade()?.read(cx).project(cx)
    }

    fn machine(&self, cx: &App) -> Option<String> {
        self.0.upgrade()?.read(cx).machine_host(cx)
    }

    fn folder_missing(&self, cx: &App) -> bool {
        self.0
            .upgrade()
            .is_some_and(|w| w.read(cx).folder_missing(cx))
    }

    fn agent_shown(&self, cx: &App) -> bool {
        self.0
            .upgrade()
            .is_some_and(|w| w.read(cx).agent_shown.is_some())
    }

    fn has_queued(&self, cx: &App) -> bool {
        self.0
            .upgrade()
            .is_some_and(|w| !w.read(cx).queued().is_empty())
    }

    fn editing_queued(&self, cx: &App) -> bool {
        self.0
            .upgrade()
            .is_some_and(|w| w.read(cx).editing_queued())
    }

    fn answering(&self, cx: &App) -> bool {
        self.0
            .upgrade()
            .is_some_and(|w| w.read(cx).answering_in_composer())
    }

    fn focus(&self, cx: &App) -> Option<FocusHandle> {
        Some(self.0.upgrade()?.read(cx).focus.clone())
    }

    fn worktree(&self, t: &Theme, cx: &mut App) -> WorktreeParts {
        let Some(workspace) = self.0.upgrade() else {
            return WorktreeParts {
                strip: None,
                files: None,
                pick: gpui::div().into_any_element(),
                told: None,
            };
        };
        workspace.update(cx, |w, cx| {
            let (strip, files) = worktree_strip(w, t, cx);
            WorktreeParts {
                strip: strip.map(IntoElement::into_any_element),
                files: files.map(IntoElement::into_any_element),
                pick: worktree_pick_chip(w, t, cx).into_any_element(),
                told: worktree_told_line(w, t, cx).map(IntoElement::into_any_element),
            }
        })
    }

    fn answer(&self, answer: String, cx: &mut App) {
        let _ = self.0.update(cx, |w, cx| w.answer_question(answer, cx));
    }

    fn send_to_agent(
        &self,
        text: String,
        attachments: Vec<Attachment>,
        new: NewChat,
        cx: &mut App,
    ) -> bool {
        self.0
            .update(cx, |w, cx| w.send_to_agent(text, attachments, new, cx))
            .unwrap_or(false)
    }

    fn send(&self, message: Outgoing, window: &mut Window, cx: &mut App) {
        let _ = self.0.update(cx, |w, cx| {
            let Outgoing {
                text,
                attachments,
                new,
                role,
                queue,
            } = message;
            // A granular chat is asked what it runs with before its first message goes, unless
            // its project says.
            if w.chat.is_none() && w.core.asks_granular(new.project_id.as_deref()) {
                w.start_granular_chat(new, role, (text, attachments, queue), window, cx);
            } else {
                w.send_with(text, attachments, Some(new), role, queue, cx);
            }
        });
    }
}

impl Workspace {
    /// The composer, with its view of this workspace and its requests answered here.
    pub(crate) fn new_composer(
        workspace: WeakEntity<Self>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Composer> {
        let composer = cx.new(|cx| Composer::new(Rc::new(Host(workspace)), window, cx));
        cx.subscribe_in(&composer, window, Self::on_composer)
            .detach();
        composer
    }

    fn on_composer(
        &mut self,
        _: &Entity<Composer>,
        event: &ChatEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ChatEvent::Stop => self.stop(cx),
            ChatEvent::SteerFirstQueued => self.steer_first_queued(cx),
            ChatEvent::EditLatestQueued => self.edit_latest_queued(cx),
            ChatEvent::UpdateQueued => self.update_queued(cx),
            ChatEvent::DismissUsage => self.dismiss_usage(cx),
            ChatEvent::Command(command) => self.send_command(command, cx),
            ChatEvent::NewChat => self.new_chat(cx),
            ChatEvent::SelectProject(Some(id)) => self.select_project(id, cx),
            ChatEvent::SelectProject(None) => {
                self.remember_project(None, cx);
                self.new_chat(cx);
            }
            ChatEvent::Reload => self.reload(cx),
            ChatEvent::OpenSettings(page) => self.open_settings(Some(page), window, cx),
        }
    }
}
