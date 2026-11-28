//! What the sidebar reads from the window that holds it, and what it asks of it.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;

use gpui::{AnyView, App, EventEmitter, Pixels, Point};
use patchwork_client::local::core::projects::Project;
use patchwork_client::local::core::worktree::WorktreeMark;

use crate::Sidebar;
use crate::rows::Row;

/// The sidebar's default width.
pub const SIDEBAR_WIDTH: f32 = 300.;

/// The narrowest a drag makes the sidebar.
pub const SIDEBAR_MIN: f32 = 200.;

/// The widest a drag makes the sidebar.
pub const SIDEBAR_MAX: f32 = 480.;

/// The sidebar's views, one per activity bar icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// The projects, their folders and tasks, and the agents.
    Tasks,
    /// Source Control's changes.
    Changes,
    /// What was archived.
    Archived,
}

/// What the sidebar reads from the window that holds it, as it is when the sidebar draws.
pub trait SidebarHost {
    /// The view the sidebar shows, or None while it is closed.
    fn view(&self, cx: &App) -> Option<View>;

    /// The sidebar's width.
    fn width(&self, cx: &App) -> f32;

    /// The open chat's id.
    fn open_chat(&self, cx: &App) -> Option<String>;

    /// The agent whose page shows in place of the chat.
    fn agent_shown(&self, cx: &App) -> Option<String>;

    /// The project the window is about: the open chat's, else the active one.
    fn project(&self, cx: &App) -> Option<Project>;

    /// The machine picked in the toolbar, by its SSH alias, or None for this computer.
    fn machine(&self, cx: &App) -> Option<String>;

    /// What task `chat`'s worktree needs from the user.
    fn worktree_mark(&self, chat: &str, cx: &App) -> Option<WorktreeMark>;

    /// Source Control's view, which the Changes view shows.
    fn changes(&self, cx: &App) -> Option<AnyView>;

    /// The usage button at the sidebar's foot.
    fn usage(&self, cx: &App) -> Option<AnyView>;
}

/// What the sidebar asks of the window that holds it.
#[derive(Debug, Clone, PartialEq)]
pub enum SidebarEvent {
    /// The search button was pressed.
    Search,
    /// Folders were dropped on the task list, each to become a project.
    AddFolders(Vec<PathBuf>),
    /// A drag of the sidebar's edge started at this x.
    StartResize(f32),
    /// The sidebar's edge was moved with the keyboard, or reset, to this width.
    SetWidth(f32),
    /// A row was right-clicked: its menu opens at `at`. The press's default was prevented, so
    /// the menu keeps the focus it takes.
    OpenMenu {
        /// The row, or the picked rows, the menu acts on.
        row: Row,
        /// Where the pointer pressed.
        at: Point<Pixels>,
    },
    /// A chat's row was clicked.
    OpenChat(String),
    /// A chat's archive button was pressed.
    Archive(String),
    /// A project's or folder's new task button was pressed.
    NewChat {
        /// The project the task goes in.
        project: Option<String>,
        /// The folder in it the task goes in.
        folder: Option<String>,
    },
    /// The open folder button was pressed.
    OpenFolder,
    /// The new agent button was pressed.
    NewAgent,
    /// An agent's row was clicked.
    OpenAgent(String),
    /// An agent's Delete key was pressed.
    DeleteAgent(String),
    /// An agent was archived or restored.
    SetAgentArchived {
        /// The agent's id.
        id: String,
        /// Whether it is archived now.
        archived: bool,
    },
    /// An agent was renamed in place.
    RenameAgent {
        /// The agent's id.
        id: String,
        /// Its new name.
        name: String,
    },
    /// A task should start in the open project.
    StartTask {
        /// The task's title.
        title: String,
        /// What the agent is asked.
        prompt: String,
    },
    /// Something the chats or the settings hold changed, so the window reads them again.
    Changed,
    /// The rename field closed while it had the focus, which goes back to the window.
    FocusWindow,
}

impl EventEmitter<SidebarEvent> for Sidebar {}
