//! The sidebar: made with its view of the window, and what the window does when it asks.

// Rust guideline compliant 2026-10-09

use patchwork_desktop_chat::prelude::*;
use std::rc::Rc;

use gpui::{AnyView, App, Entity, WeakEntity, Window, prelude::*};
use patchwork_core::projects::Project;
use patchwork_core::worktree::WorktreeMark;
use patchwork_desktop_sidebar::{Sidebar, SidebarEvent, SidebarHost, View};

use crate::window::Workspace;

/// The sidebar's view of the workspace: what is open, the project, and the views it shows.
struct Host(WeakEntity<Workspace>);

impl SidebarHost for Host {
    fn view(&self, cx: &App) -> Option<View> {
        self.0.upgrade()?.read(cx).sidebar_view()
    }

    fn width(&self, cx: &App) -> f32 {
        self.0
            .upgrade()
            .map_or(patchwork_desktop_sidebar::SIDEBAR_WIDTH, |w| {
                w.read(cx).sidebar_width
            })
    }

    fn open_chat(&self, cx: &App) -> Option<String> {
        let workspace = self.0.upgrade()?;
        workspace.read(cx).chat.as_ref().map(|t| t.id.clone())
    }

    fn agent_shown(&self, cx: &App) -> Option<String> {
        self.0.upgrade()?.read(cx).agent_shown.clone()
    }

    fn project(&self, cx: &App) -> Option<Project> {
        self.0.upgrade()?.read(cx).project(cx).cloned()
    }

    fn machine(&self, cx: &App) -> Option<String> {
        self.0.upgrade()?.read(cx).machine_host(cx)
    }

    fn worktree_mark(&self, chat: &str, cx: &App) -> Option<WorktreeMark> {
        self.0.upgrade()?.read(cx).worktree_marks.get(chat).copied()
    }

    fn changes(&self, cx: &App) -> Option<AnyView> {
        Some(self.0.upgrade()?.read(cx).changes_view.clone().into())
    }

    fn usage(&self, cx: &App) -> Option<AnyView> {
        Some(self.0.upgrade()?.read(cx).usage.clone().into())
    }
}

impl Workspace {
    /// Makes the sidebar, which draws again whenever the workspace does.
    pub(crate) fn new_sidebar(window: &mut Window, cx: &mut Context<Self>) -> Entity<Sidebar> {
        let workspace = cx.entity();
        let host = Rc::new(Host(workspace.downgrade()));
        let sidebar = cx.new(|cx| {
            cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
            Sidebar::new(host, cx)
        });
        cx.subscribe_in(&sidebar, window, Self::on_sidebar).detach();
        sidebar
    }

    /// Does what the sidebar asks.
    fn on_sidebar(
        &mut self,
        _: &Entity<Sidebar>,
        event: &SidebarEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            SidebarEvent::Search => self.toggle_palette(window, cx),
            SidebarEvent::AddFolders(paths) => self.add_dropped_folders(paths, cx),
            SidebarEvent::StartResize(x) => self.start_resize(*x),
            SidebarEvent::SetWidth(width) => {
                self.sidebar_width = *width;
                cx.notify();
            }
            SidebarEvent::OpenMenu { row, at } => {
                self.open_context_menu(row.clone().into(), *at, window, cx);
            }
            SidebarEvent::OpenChat(id) => self.open_chat(id, cx),
            SidebarEvent::Archive(id) => self.archive(id, cx),
            SidebarEvent::NewChat { project, folder } => {
                self.new_chat_in(project.clone(), folder.as_deref(), window, cx);
            }
            SidebarEvent::OpenFolder => self.open_folder(window, cx),
            SidebarEvent::NewAgent => self.new_agent(cx),
            SidebarEvent::OpenAgent(id) => self.open_agent(id, cx),
            SidebarEvent::DeleteAgent(id) => self.delete_agent(id, window, cx),
            SidebarEvent::SetAgentArchived { id, archived } => {
                self.set_agent_archived(id, *archived, cx);
            }
            SidebarEvent::RenameAgent { id, name } => {
                self.rename_agent(id.clone(), name.clone(), cx);
            }
            SidebarEvent::StartTask { title, prompt } => self.start_task(title, prompt.clone(), cx),
            SidebarEvent::Changed => self.reload(cx),
            SidebarEvent::FocusWindow => window.focus(&self.focus, cx),
        }
    }
}
