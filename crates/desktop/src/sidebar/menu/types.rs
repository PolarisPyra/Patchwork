//! The rows a menu or a name field acts on, and the open menu and name field themselves.

// Rust guideline compliant 2026-10-09

use gpui::{Entity, FocusHandle, Pixels, Point, SharedString};
use crate::ui::text_area::TextArea;

/// A row the menu or the name field acts on.
#[derive(Clone, PartialEq, Eq)]
pub enum Target {
    Thread(String),
    Project(String),
    Folder(String),
    /// An archived view header: the archived chats or tasks under it.
    Archived(Vec<String>),
    /// Rows picked together with Ctrl or Shift.
    Many(Vec<Target>),
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

#[derive(Clone, Copy)]
pub(super) enum Act {
    Rename,
    Favorite(bool),
    Pin(bool),
    Background,
    Granular,
    Export,
    OpenLogs,
    WorktreeTask,
    ChangeFolder,
    OpenFolder,
    Remove,
    Restore,
}

pub(super) struct Item {
    pub(super) label: SharedString,
    pub(super) icon: &'static str,
    pub(super) danger: bool,
    pub(super) act: Act,
}

/// Where each task of a project was before it was sorted by provider, in order.
pub type SortUndo = Vec<(String, Option<String>)>;
