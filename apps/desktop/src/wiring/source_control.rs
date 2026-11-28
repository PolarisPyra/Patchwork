//! Source Control in the window: its view of the open project and chat, and what it asks of the
//! workspace.

// Rust guideline compliant 2026-10-09

use patchwork_desktop_chat::prelude::*;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{App, Entity, WeakEntity, Window, prelude::*};
use patchwork_core::chats::Chat;
use patchwork_core::orchestration::changes::turn_files::TurnFileState;
use patchwork_core::source_control::FileChange;
use patchwork_desktop_source_control::{ChangesView, SourceControlEvent, SourceControlHost};
use patchwork_client::GitModel;
use patchwork_client::AppState;

use crate::window::Workspace;

/// Source Control's view of the workspace: the project's changes and the open chat.
struct Host(WeakEntity<Workspace>);

impl SourceControlHost for Host {
    fn changes<'a>(&self, cx: &'a App) -> &'a [FileChange] {
        &GitModel::read(cx).changes
    }

    fn open_chat<'a>(&self, cx: &'a App) -> Option<&'a Chat> {
        self.0.upgrade()?.read(cx).chat.as_ref()
    }

    fn git_root<'a>(&self, cx: &'a App) -> Option<&'a str> {
        GitModel::read(cx).git_root.as_deref()
    }

    fn has_project(&self, cx: &App) -> bool {
        self.0
            .upgrade()
            .is_some_and(|w| w.read(cx).project(cx).is_some())
    }

    fn change_focus(&self, cx: &App) -> Option<(String, String)> {
        GitModel::read(cx).change_focus.clone()
    }

    fn reply_files(&self, cx: &App) -> Vec<String> {
        let Some(workspace) = self.0.upgrade() else {
            return Vec::new();
        };
        let w = workspace.read(cx);
        let Some((chat, message)) = &GitModel::read(cx).change_focus else {
            return Vec::new();
        };
        AppState::get(cx)
            .chats
            .iter()
            .map(Arc::as_ref)
            .chain(w.chat.iter())
            .find(|t| &t.id == chat)
            .and_then(|t| t.messages.iter().find(|m| &m.id == message))
            .map(|m| m.files.clone())
            .unwrap_or_default()
    }

    fn turn_file_state(&self, path: &str, cx: &App) -> Option<TurnFileState> {
        let git = GitModel::read(cx);
        let (_, message) = git.change_focus.as_ref()?;
        let files = git.reply_files.get(message)?.turn_files.as_ref()?;
        files.iter().find(|f| f.path == path).map(|f| f.state)
    }
}

impl Workspace {
    /// Makes Source Control for the window, and handles what it asks of the workspace.
    pub(crate) fn new_changes_view(
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ChangesView> {
        let host: Rc<dyn SourceControlHost> = Rc::new(Host(cx.entity().downgrade()));
        let view = cx.new(|cx| ChangesView::new(host, window, cx));
        cx.subscribe_in(&view, window, Self::on_source_control)
            .detach();
        view
    }

    /// Does what Source Control asks.
    fn on_source_control(
        &mut self,
        _: &Entity<ChangesView>,
        event: &SourceControlEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            SourceControlEvent::LoadGit => self.load_git(cx),
            SourceControlEvent::StartTask { title, text } => {
                self.start_task(title, text.clone(), cx);
            }
            SourceControlEvent::SendTo { chat, prompt } => self.send_to(chat, prompt.clone(), cx),
            SourceControlEvent::OpenChat(id) => self.open_chat(id, cx),
            SourceControlEvent::OpenChange { path, side: None } => {
                self.open_change(path, window, cx);
            }
            SourceControlEvent::OpenChange {
                path,
                side: Some(side),
            } => self.open_change_side(path, *side, window, cx),
            SourceControlEvent::OpenChangeSince { path, since } => {
                self.open_change_since(path, since.clone(), window, cx);
            }
            SourceControlEvent::ClearChangeFocus => {
                GitModel::update(cx, |git, _| git.change_focus = None);
            }
        }
    }
}
