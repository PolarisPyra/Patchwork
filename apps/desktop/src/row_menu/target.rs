//! The rows a menu acts on, and the open menu itself.

// Rust guideline compliant 2026-10-09

use gpui::{FocusHandle, Pixels, Point, SharedString};
use patchwork_core::gateway::Platform;
use patchwork_desktop_sidebar::Row;
use patchwork_design_icons::IconName;

/// A row the menu acts on.
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
    Pane(patchwork_desktop_chat::panes::PaneId),
}

/// The open menu: where it was opened, for which row, and the row the keyboard is on.
pub struct ContextMenu {
    pub(super) at: Point<Pixels>,
    pub(super) target: Target,
    pub(super) focus: FocusHandle,
    pub(super) highlight: Option<usize>,
}

#[derive(Clone)]
pub(super) enum Act {
    Rename,
    /// Put the chat in a tab beside the open chat's.
    OpenInNewTab,
    /// Take the chat out of the tabs.
    CloseTab,
    /// Show the chat in a new pane at this edge of the focused one.
    Split(patchwork_desktop_chat::panes::Zone),
    /// Close the chat's other tabs in its pane.
    CloseOthers,
    /// Close the tabs after the chat's in its pane.
    CloseRight,
    /// Split the pane at this edge.
    SplitPane(patchwork_desktop_chat::panes::Zone),
    /// Show the pane alone, or every pane again.
    Maximize,
    /// Trade places with this pane.
    SwapPane(patchwork_desktop_chat::panes::PaneId),
    /// Close the pane and its tabs.
    ClosePane,
    /// Send each message to every pane, or stop.
    Broadcast,
    /// Open the picked chats each in a pane, side by side.
    SideBySide,
    /// Lay the panes out as this.
    Preset(patchwork_desktop_chat::panes::Preset),
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
    pub(super) icon: IconName,
    pub(super) danger: bool,
    pub(super) act: Act,
}

impl From<Row> for Target {
    fn from(row: Row) -> Self {
        match row {
            Row::Chat(id) => Self::Chat(id),
            Row::Project(id) => Self::Project(id),
            Row::Folder(id) => Self::Folder(id),
            Row::Agent(id) => Self::Agent(id),
            Row::Archived(ids) => Self::Archived(ids),
            Row::Many(rows) => Self::Many(rows.into_iter().map(Self::from).collect()),
        }
    }
}

impl Target {
    /// The sidebar's row this is, or None for a tab strip's or a pane's.
    pub(super) fn row(&self) -> Option<Row> {
        Some(match self {
            Self::Chat(id) => Row::Chat(id.clone()),
            Self::Project(id) => Row::Project(id.clone()),
            Self::Folder(id) => Row::Folder(id.clone()),
            Self::Agent(id) => Row::Agent(id.clone()),
            Self::Archived(ids) => Row::Archived(ids.clone()),
            Self::Many(targets) => Row::Many(targets.iter().filter_map(Self::row).collect()),
            Self::Tabs(_) | Self::Layouts | Self::Pane(_) => return None,
        })
    }
}
