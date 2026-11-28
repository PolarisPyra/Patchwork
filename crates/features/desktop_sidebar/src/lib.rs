//! Patchwork's sidebar: the search button, its views (the task tree, Source Control's changes, the
//! open project's issues, and what was archived), the usage button, and the edge that resizes it.
//!
//! The sidebar reads the chats, projects and agents from [`AppState`], and what the window shows
//! through a [`SidebarHost`]. What needs the window, such as opening a chat or a row's menu, it
//! asks for with a [`SidebarEvent`].

#![expect(
    clippy::unused_self,
    reason = "a view acts through its `Context<Self>`, so some of its methods never read the view itself"
)]

// Rust guideline compliant 2026-10-09

mod agents_section;
mod archived_list;
mod chat_row;
mod collapsed;
mod frame;
mod host;
mod issues;
mod provider_folders;
mod rename_field;
mod reorder;
mod rows;
mod task_list;

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{Empty, MouseDownEvent, ScrollHandle, Subscription, Window, prelude::*};
use patchwork_client::local::core::Core;
use patchwork_model::chats::Chat;
use patchwork_client::AppState;

pub use host::{SIDEBAR_MAX, SIDEBAR_MIN, SIDEBAR_WIDTH, SidebarEvent, SidebarHost, View};
pub use rows::Row;

use issues::IssueLists;
use provider_folders::SortUndo;
use rename_field::Renaming;
use rows::Picking;

/// The sidebar: the views the activity bar switches between, and what they keep between frames.
pub struct Sidebar {
    core: Arc<Core>,
    host: Rc<dyn SidebarHost>,
    /// The projects and folders folded away, saved between launches. A folder is `folder:{id}`.
    collapsed: HashSet<String>,
    /// The Archived view's groups folded away, for this launch.
    archived_collapsed: HashSet<String>,
    issues: IssueLists,
    picking: Picking,
    renaming: Option<Renaming>,
    /// Where each sorted project's tasks were before it was sorted by provider.
    sort_undo: HashMap<String, SortUndo>,
    /// The chat row under the pointer, which trades its time for its archive button.
    hovered_row: Option<String>,
    /// The task a dragged task would land before.
    task_drop_before: Option<String>,
    tree_scroll: ScrollHandle,
    _state: Subscription,
}

impl fmt::Debug for Sidebar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sidebar")
            .field("collapsed", &self.collapsed)
            .field("archived_collapsed", &self.archived_collapsed)
            .field("picking", &self.picking)
            .field("hovered_row", &self.hovered_row)
            .field("task_drop_before", &self.task_drop_before)
            .finish_non_exhaustive()
    }
}

impl Sidebar {
    /// A sidebar that reads what the window shows through `host`, and redraws when the app's
    /// state changes.
    pub fn new(host: Rc<dyn SidebarHost>, cx: &mut Context<Self>) -> Self {
        let core = Arc::clone(AppState::get(cx).core());
        let collapsed = core.collapsed_sections().into_iter().collect();
        Self {
            core,
            host,
            collapsed,
            archived_collapsed: HashSet::new(),
            issues: IssueLists::default(),
            picking: Picking::default(),
            renaming: None,
            sort_undo: HashMap::new(),
            hovered_row: None,
            task_drop_before: None,
            tree_scroll: ScrollHandle::new(),
            _state: cx.observe_global::<AppState>(|_, cx| cx.notify()),
        }
    }

    /// Makes a change to what is stored off the UI thread, which a write to the database would
    /// hold up, then has the window read it. `what` names the change in the log if it fails.
    fn change(
        &mut self,
        what: &'static str,
        change: impl FnOnce(&Arc<Core>) -> Result<(), patchwork_client::local::core::Error>
        + Send
        + 'static,
        cx: &mut Context<Self>,
    ) {
        let core = Arc::clone(&self.core);
        patchwork_design_tasks::off_thread(
            cx,
            move || change(&core),
            move |this, done, cx| {
                if let Err(e) = done {
                    log::warn!(what, error:% = e; "could not {{what}}: {{error}}");
                }
                // A project's name is in the settings.
                AppState::update(cx, AppState::reload_settings);
                this.reload(cx);
            },
        );
    }

    /// Has the window read the chats and the settings again.
    fn reload(&mut self, cx: &mut Context<Self>) {
        cx.emit(SidebarEvent::Changed);
        cx.notify();
    }

    /// Chat `id` as the app state holds it.
    fn chat_by_id(&self, id: &str, cx: &gpui::App) -> Option<Arc<Chat>> {
        AppState::get(cx).chats.iter().find(|c| c.id == id).cloned()
    }

    /// Opens `row`'s menu where the pointer pressed. The press's default is prevented here, while
    /// it is still being handled, so the menu keeps the focus it takes.
    fn open_menu(
        &mut self,
        row: Row,
        e: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.prevent_default();
        cx.emit(SidebarEvent::OpenMenu {
            row,
            at: e.position,
        });
        cx.notify();
    }

    fn open_chat(&mut self, id: &str, cx: &mut Context<Self>) {
        cx.emit(SidebarEvent::OpenChat(id.to_owned()));
    }

    fn archive(&mut self, id: &str, cx: &mut Context<Self>) {
        cx.emit(SidebarEvent::Archive(id.to_owned()));
    }

    fn new_chat_in(
        &mut self,
        project: Option<String>,
        folder: Option<String>,
        cx: &mut Context<Self>,
    ) {
        cx.emit(SidebarEvent::NewChat { project, folder });
    }

    fn open_folder(&mut self, cx: &mut Context<Self>) {
        cx.emit(SidebarEvent::OpenFolder);
    }

    fn new_agent(&mut self, cx: &mut Context<Self>) {
        cx.emit(SidebarEvent::NewAgent);
    }

    fn open_agent(&mut self, id: &str, cx: &mut Context<Self>) {
        cx.emit(SidebarEvent::OpenAgent(id.to_owned()));
    }

    fn delete_agent(&mut self, id: &str, cx: &mut Context<Self>) {
        cx.emit(SidebarEvent::DeleteAgent(id.to_owned()));
    }

    fn set_agent_archived(&mut self, id: &str, archived: bool, cx: &mut Context<Self>) {
        cx.emit(SidebarEvent::SetAgentArchived {
            id: id.to_owned(),
            archived,
        });
    }

    fn add_dropped_folders(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        cx.emit(SidebarEvent::AddFolders(paths.to_vec()));
    }

    /// Opens the project and folder chat `id` is in, so its row shows in the task tree.
    pub fn expand_to(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(chat) = self.chat_by_id(id, cx) else {
            return;
        };
        let mut opened = false;
        if let Some(project) = &chat.project_id {
            opened |= self.collapsed.remove(project);
        }
        if let Some(folder) = &chat.folder_id {
            opened |= self.collapsed.remove(&format!("folder:{folder}"));
        }
        if opened {
            self.save_collapsed(cx);
            cx.notify();
        }
    }

    /// A project was added: it starts closed like the rest of the list. Call it before the
    /// settings are read again, so a project already listed keeps its state.
    pub fn project_added(&mut self, id: &str, cx: &mut Context<Self>) {
        let listed = AppState::get(cx)
            .settings
            .projects
            .iter()
            .any(|p| p.id == id);
        if !listed {
            self.collapsed.insert(id.to_owned());
            self.save_collapsed(cx);
        }
    }
}

impl Render for Sidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.host.view(cx) {
            Some(view) => frame::render(self, view, cx).into_any_element(),
            None => Empty.into_any_element(),
        }
    }
}
