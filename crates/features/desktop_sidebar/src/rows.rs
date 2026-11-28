//! The rows of the task tree a menu or the name field acts on, and picking several of them:
//! Ctrl+click adds or drops a row, Shift+click takes every row from the last one clicked, and the
//! right-click menu then acts on all of them.

// Rust guideline compliant 2026-10-09

use gpui::{Context, Modifiers};

use crate::Sidebar;

/// A row of the sidebar's trees.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// A chat or task, by its id.
    Chat(String),
    /// A project, by its id.
    Project(String),
    /// A folder in a project, by its id.
    Folder(String),
    /// An agent, by its id.
    Agent(String),
    /// An archived view header: the archived chats or tasks under it.
    Archived(Vec<String>),
    /// Rows picked together with Ctrl or Shift.
    Many(Vec<Row>),
}

/// The rows picked together, and the row last clicked.
#[derive(Debug, Default)]
pub(crate) struct Picking {
    /// The rows picked with Ctrl or Shift, in tree order for a Shift range.
    pub(crate) rows: Vec<Row>,
    /// The row last clicked, where a Shift range starts.
    cursor: Option<Row>,
    /// The tree's rows as last drawn, top to bottom, for a Shift range to run along.
    pub(crate) tree: Vec<Row>,
    /// The row last plainly clicked, which the Delete key acts on. Any other press lets it go.
    pub(crate) clicked: Option<Row>,
}

impl Sidebar {
    /// Ctrl+click adds or drops `row` from the picked rows, and Shift+click picks the rows from
    /// the last one clicked; both return true. A plain click clears them and returns false, so the
    /// row acts as usual.
    pub(crate) fn pick_row(&mut self, row: &Row, m: Modifiers, cx: &mut Context<Self>) -> bool {
        let picking = &mut self.picking;
        let anchor = picking
            .cursor
            .as_ref()
            .and_then(|c| picking.tree.iter().position(|r| r == c));
        let at = picking.tree.iter().position(|r| r == row);
        picking.cursor = Some(row.clone());
        if m.shift
            && let (Some(anchor), Some(at)) = (anchor, at)
        {
            let (from, to) = if anchor < at {
                (anchor, at)
            } else {
                (at, anchor)
            };
            picking.rows = picking.tree[from..=to].to_vec();
            cx.notify();
            return true;
        }
        if !(m.control || m.platform) {
            picking.clicked = Some(row.clone());
            if !picking.rows.is_empty() {
                picking.rows.clear();
                cx.notify();
            }
            return false;
        }
        match picking.rows.iter().position(|r| r == row) {
            Some(i) => {
                picking.rows.remove(i);
            }
            None => picking.rows.push(row.clone()),
        }
        cx.notify();
        true
    }

    /// What a right-click on `row` acts on: every picked row when it is one of them, otherwise
    /// it alone, and the others are let go.
    pub(crate) fn menu_row(&mut self, row: Row) -> Row {
        if self.picking.rows.len() > 1 && self.picking.rows.contains(&row) {
            return Row::Many(self.picking.rows.clone());
        }
        self.picking.rows.clear();
        row
    }

    /// Whether `row` is one of the picked rows.
    pub(crate) fn is_picked(&self, row: &Row) -> bool {
        self.picking.rows.contains(row)
    }

    /// The rows picked with Ctrl or Shift.
    #[must_use]
    pub fn picked(&self) -> Vec<Row> {
        self.picking.rows.clone()
    }

    /// The row last plainly clicked, which the Delete key acts on.
    #[must_use]
    pub fn clicked(&self) -> Option<Row> {
        self.picking.clicked.clone()
    }

    /// Lets go of the row last clicked, as any press elsewhere does.
    pub fn clear_clicked(&mut self) {
        self.picking.clicked = None;
    }

    /// Lets go of the picked rows.
    pub fn clear_picked(&mut self, cx: &mut Context<Self>) {
        if !self.picking.rows.is_empty() {
            self.picking.rows.clear();
            cx.notify();
        }
    }
}
