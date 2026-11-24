//! The rows a menu or a name field acts on, and the open menu and name field themselves.

// Rust guideline compliant 2026-10-09

use gpui::{Entity, FocusHandle, Pixels, Point, SharedString};
use patchwork_core::gateway::Platform;
use patchwork_ui::text::text_area::TextArea;

/// A row the menu or the name field acts on.
#[derive(Clone, PartialEq, Eq)]
pub enum Target {
    Chat(String),
    Project(String),
    Folder(String),
    Agent(String),
    /// An archived view header: the archived chats or tasks under it.
    Archived(Vec<String>),
    /// Rows picked together with Ctrl or Shift.
    Many(Vec<Target>),
    /// The chat tabs' add button, for the project (or none) whose tabs show.
    Tabs(Option<String>),
    /// The layout button over the panes of the project (or none) whose tabs show.
    Layouts,
    /// A pane's tab strip, away from its tabs.
    Pane(crate::chat::panes::PaneId),
}

/// The open menu: where it was opened, for which row, and the row the keyboard is on.
pub struct ContextMenu {
    pub(super) at: Point<Pixels>,
    pub(super) target: Target,
    pub(super) focus: FocusHandle,
    pub(super) highlight: Option<usize>,
}

/// The name field in place of a row's name.
pub struct Renaming {
    pub target: Target,
    pub field: Entity<TextArea>,
    /// What it was called, so an unchanged name saves nothing.
    pub(super) current: String,
}

#[derive(Clone)]
pub(super) enum Act {
    Rename,
    /// Put the chat in a tab beside the open chat's.
    OpenInNewTab,
    /// Take the chat out of the tabs.
    CloseTab,
    /// Show the chat in a new pane at this edge of the focused one.
    Split(crate::chat::panes::Zone),
    /// Close the chat's other tabs in its pane.
    CloseOthers,
    /// Close the tabs after the chat's in its pane.
    CloseRight,
    /// Split the pane at this edge.
    SplitPane(crate::chat::panes::Zone),
    /// Show the pane alone, or every pane again.
    Maximize,
    /// Trade places with this pane.
    SwapPane(crate::chat::panes::PaneId),
    /// Close the pane and its tabs.
    ClosePane,
    /// Send each message to every pane, or stop.
    Broadcast,
    /// Open the picked chats each in a pane, side by side.
    SideBySide,
    /// Lay the panes out as this.
    Preset(crate::chat::panes::Preset),
    /// Lay the panes out as the layout the user named this.
    Layout(String),
    /// Start a task in a new tab.
    NewTaskTab,
    /// Open this task in a new tab.
    OpenTab(String),
    Favorite(bool),
    Pin(bool),
    Background,
    Granular,
    Export,
    OpenLogs,
    WorktreeTask,
    /// New tasks in the project edit its folder (true) or get a worktree each (false).
    EditInPlace(bool),
    ChangeFolder,
    OpenFolder,
    Remove,
    Restore,
    /// Show an agent's page: its instructions, files, tools and runs.
    Details,
    /// Stop continuing the chat in this conversation on a chat app.
    StopOn(Platform, String),
}

pub(super) struct Item {
    pub(super) label: SharedString,
    pub(super) icon: &'static str,
    pub(super) danger: bool,
    pub(super) act: Act,
}

/// Where each task of a project was before it was sorted by provider, in order.
pub type SortUndo = Vec<(String, Option<String>)>;
